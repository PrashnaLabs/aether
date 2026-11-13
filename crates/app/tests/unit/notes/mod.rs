use super::*;
use aether_core::{Provider, Settings};
use gpui_kit::{TestAppContext, WindowHandle};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    dir: PathBuf,
    project: PathBuf,
    notes: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "aether-notes-ui-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = aether_core::canonicalize(&dir).unwrap();
        let project = dir.join("project");
        let notes = dir.join("notes");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&notes).unwrap();
        Self { dir, project, notes }
    }
    fn window(&self, cx: &mut TestAppContext) -> WindowHandle<ChatApp> {
        cx.background_executor.allow_parking();
        cx.update(gpui_kit::init);
        let mut settings = Settings {
            provider: Some(Provider::OpenRouter),
            workspace_root: self.project.display().to_string(),
            ..Settings::default()
        };
        settings.notes.directory = self.notes.display().to_string();
        cx.add_window(|window, cx| {
            ChatApp::with_state(
                aether_agent::Agent::new().unwrap(),
                settings,
                None,
                None,
                false,
                window,
                cx,
            )
        })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[gpui_kit::test]
fn notes_navigation_keeps_the_project_and_its_editor_separate(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let project_file = f.project.join("agent.txt");
    let note_file = f.notes.join("native.md");
    std::fs::write(&project_file, "project buffer").unwrap();
    std::fs::write(&note_file, "# Note\n").unwrap();
    let window = f.window(cx);
    window
        .update(cx, |app, window, cx| {
            app.open_explorer_file(project_file.clone(), window, cx);
            let project_folder = app.active_project_folder();
            let project_explorer = app.active_project_explorer(cx);
            app.open_notes(window, cx);
            assert_eq!(app.page, Page::Notes);
            assert_eq!(app.active_project_folder(), project_folder);
            let notes_explorer = app.notes.explorer.as_ref().unwrap().clone();
            assert_ne!(project_explorer.entity_id(), notes_explorer.entity_id());
            assert_eq!(notes_explorer.read(cx).root, f.notes);
            app.open_explorer_file(note_file.clone(), window, cx);
            assert_eq!(app.page, Page::Notes, "native notes stay on the Notes page");
            assert_eq!(notes_explorer.read(cx).tabs[0].path, note_file);
            assert_eq!(project_explorer.read(cx).tabs[0].path, project_file);
            assert_eq!(project_explorer.read(cx).tabs.len(), 1);
            assert_eq!(app.active_project_folder(), project_folder);
        })
        .unwrap();
}

#[gpui_kit::test]
fn dirty_notes_block_root_changes_without_losing_buffers(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let note_file = f.notes.join("unsaved.txt");
    std::fs::write(&note_file, "disk text").unwrap();
    let window = f.window(cx);
    window
        .update(cx, |app, window, cx| {
            app.open_notes(window, cx);
            app.open_explorer_file(note_file.clone(), window, cx);
            let explorer = app.notes.explorer.as_ref().unwrap().clone();
            explorer.update(cx, |exp, _| exp.tabs[0].is_dirty = true);
            let generation = app.notes.generation;
            let original = app.settings.notes.directory.clone();
            let mut proposed = app.settings.notes.clone();
            proposed.directory = f.dir.join("different-notes").display().to_string();
            app.update_notes_settings(proposed, cx);
            assert_eq!(app.settings.notes.directory, original);
            assert_eq!(app.notes.generation, generation);
            assert_eq!(app.notes.explorer.as_ref().unwrap().entity_id(), explorer.entity_id());
            assert!(explorer.read(cx).tabs[0].is_dirty);
            assert!(app.notes.error.as_ref().unwrap().contains("unsaved"));
            assert_eq!(std::fs::read_to_string(&note_file).unwrap(), "disk text");
        })
        .unwrap();
}

#[gpui_kit::test]
fn capture_form_retains_original_text_and_store_appends_it_exactly(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let original = "  selected text\n\n```rust\nlet x = 1;\n```\ntrailing spaces  \n";
    let window = f.window(cx);
    window
        .update(cx, |app, window, cx| {
            app.notes_form(Form::Capture, original.into(), window, cx);
            assert!(matches!(app.notes.form, Some(Form::Capture)));
            assert_eq!(app.notes.initial, original);
            assert_eq!(app.page, Page::Notes);
            let store = app.notes_store().unwrap().with_last_note_file(f.dir.join("last-note"));
            let path = store.capture("captured", &app.notes.initial).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
            store.capture("captured", original).unwrap();
            assert_eq!(std::fs::read_to_string(path).unwrap(), format!("{original}{original}"));
            assert!(!f.dir.join("last-note").exists(), "capture must not remember until opened");
        })
        .unwrap();
}

#[gpui_kit::test]
fn project_notes_follow_a_project_switch(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let other = f.dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let window = f.window(cx);
    window
        .update(cx, |app, window, cx| {
            app.settings.notes.project_scope = true;
            app.notes_form(Form::Capture, "a".into(), window, cx);
            assert_eq!(app.notes.project.as_deref(), Some("project"));
            app.settings.workspace_root = other.display().to_string();
            app.notes_form(Form::Capture, "b".into(), window, cx);
            assert_eq!(app.notes.project.as_deref(), Some("other"));
        })
        .unwrap();
}

#[gpui_kit::test]
fn chat_saves_notes_to_its_own_project(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let window = f.window(cx);
    window
        .update(cx, |app, window, cx| {
            app.save_chat_note("direct".into(), window, cx);
            assert_eq!(app.notes.project, None, "a direct chat saves to Global");
            app.open_page(Page::Chat, cx);
            app.chats[app.active].provider = Provider::Codex;
            app.save_chat_note("agent".into(), window, cx);
            assert_eq!(app.notes.project.as_deref(), Some("project"));
            assert!(matches!(app.notes.form, Some(Form::Capture)));
        })
        .unwrap();
}

#[gpui_kit::test]
fn toggled_project_notes_follow_a_project_switch(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let other = f.dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let window = f.window(cx);
    window
        .update(cx, |app, window, cx| {
            app.settings.notes_enabled = true;
            app.open_notes(window, cx);
            assert_eq!(app.notes.project, None);
            app.notes.project = Some("project".into());
            app.open_page(Page::Chat, cx);
            app.settings.workspace_root = other.display().to_string();
            app.open_notes(window, cx);
            assert_eq!(app.notes.project.as_deref(), Some("other"));
        })
        .unwrap();
}

#[gpui_kit::test]
fn invalidated_discovery_cannot_replace_newer_notes_results(cx: &mut TestAppContext) {
    let f = Fixture::new();
    std::fs::write(f.notes.join("old.md"), "old result").unwrap();
    let window = f.window(cx);
    let sentinel = f.notes.join("newer.md");
    window
        .update(cx, |app, _, cx| {
            app.refresh_notes(cx);
            // A root, scope, or query change invalidates the pending generation.
            app.notes.generation += 1;
            app.notes.entries = vec![(sentinel.clone(), "newer result".into())];
            app.notes.loading = false;
        })
        .unwrap();
    cx.run_until_parked();
    cx.background_executor.advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    window
        .update(cx, |app, _, _| {
            assert_eq!(app.notes.entries, vec![(sentinel.clone(), "newer result".into())]);
            assert!(!app.notes.loading);
        })
        .unwrap();
}

#[gpui_kit::test]
fn notes_page_keeps_tree_full_height_without_other_views(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let path = f.notes.join("preview.md");
    std::fs::write(&path, "# Native note\n\nA Markdown preview.\n").unwrap();
    std::fs::create_dir_all(f.notes.join("docs")).unwrap();
    let window = f.window(cx);
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.open_notes(window, cx);
            app.open_explorer_file(path.clone(), window, cx);
        });
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("explorer-markdown-preview").is_some());
    assert!(visual.debug_bounds("notes-folder-tree").is_some());
    assert!(visual.debug_bounds("notes-results").is_none());
    let tree = visual.debug_bounds("explorer-tree").expect("shared Explorer tree");
    let page = visual.debug_bounds("notes-page").unwrap();
    assert!(
        tree.size.height > page.size.height * 0.7,
        "tree must fill the sidebar: {tree:?}, page {page:?}"
    );
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.notes.search.update(cx, |input, cx| input.set_value("preview", window, cx))
        });
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("notes-folder-tree").is_some());
    assert!(visual.debug_bounds("notes-results").is_none());
    assert!(visual.debug_bounds("notes-all").is_none());
    assert!(visual.debug_bounds("notes-recent").is_none());
}

#[gpui_kit::test]
fn external_note_edits_reload_clean_buffers_and_preserve_dirty_text(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let path = f.notes.join("shared.txt");
    std::fs::write(&path, "original\n").unwrap();
    let window = f.window(cx);
    window
        .update(cx, |app, window, cx| {
            app.open_notes(window, cx);
            app.open_explorer_file(path.clone(), window, cx);
            let explorer = app.notes.explorer.as_ref().unwrap().clone();
            let editor = app.active_explorer_editor(window, cx).unwrap();
            std::fs::write(&path, "original\nexternal append\n").unwrap();
            app.check_explorer_disk(&f.notes, window, cx);
            assert_eq!(editor.read(cx).text().to_string(), "original\nexternal append\n");
            assert_eq!(explorer.read(cx).tabs[0].original, "original\nexternal append\n");
            assert!(!explorer.read(cx).tabs[0].is_dirty);
            editor.update(cx, |editor, cx| editor.replace_all("unsaved native text", window, cx));
            explorer.update(cx, |state, _| state.tabs[0].is_dirty = true);
            std::fs::write(&path, "original\nexternal append\nanother external edit\n").unwrap();
            app.check_explorer_disk(&f.notes, window, cx);
            assert_eq!(editor.read(cx).text().to_string(), "unsaved native text");
            assert!(explorer.read(cx).tabs[0].is_dirty);
            assert_eq!(
                explorer.read(cx).tabs[0].conflict,
                Some(aether_explorer::DiskConflict::Changed)
            );
            assert_eq!(
                std::fs::read_to_string(&path).unwrap(),
                "original\nexternal append\nanother external edit\n"
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn blank_and_busy_notes_root_changes_are_rejected(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let window = f.window(cx);
    window
        .update(cx, |app, _, cx| {
            let original = app.settings.notes.directory.clone();
            let generation = app.notes.generation;
            let mut proposed = app.settings.notes.clone();
            proposed.directory = " \t ".into();
            app.update_notes_settings(proposed, cx);
            assert_eq!(app.settings.notes.directory, original);
            assert!(app.notes.error.is_some());
            assert_eq!(app.notes.generation, generation);
            app.notes.busy = true;
            let mut proposed = app.settings.notes.clone();
            proposed.directory = f.dir.join("other-notes").display().to_string();
            app.update_notes_settings(proposed, cx);
            assert_eq!(app.settings.notes.directory, original);
            assert_eq!(app.notes.generation, generation);
            assert!(app.notes.error.as_ref().unwrap().contains("operation"));
            assert!(app.notes.busy);
        })
        .unwrap();
}

#[gpui_kit::test]
fn new_note_at_scope_root_has_an_empty_relative_name(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let window = f.window(cx);
    window
        .update(cx, |app, window, cx| {
            app.new_note_here(f.notes.clone(), window, cx);
            assert!(app.notes.name.read(cx).value().is_empty());
            app.notes.project = Some("my-project".into());
            let scoped_root = f.notes.join("projects/my-project");
            app.new_note_here(scoped_root.clone(), window, cx);
            assert!(app.notes.name.read(cx).value().is_empty());
            app.new_note_here(scoped_root.join("docs"), window, cx);
            assert_eq!(app.notes.name.read(cx).value().to_string(), "docs/");
            assert_eq!(app.active_project_folder(), f.project);
        })
        .unwrap();
}

#[gpui_kit::test]
fn create_note_runs_end_to_end_and_keeps_last_note_inside_fixture(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let window = f.window(cx);
    window
        .update(cx, |app, window, cx| {
            app.notes.last_note_file = Some(f.dir.join("last-note"));
            app.notes_form(Form::Capture, "# Saved selection\n".into(), window, cx);
            app.notes.name.update(cx, |input, cx| input.set_value("captured", window, cx));
            app.submit_note_form(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |app, _, cx| {
            assert!(!app.notes.busy);
            assert!(app.notes.error.is_none(), "{:?}", app.notes.error);
            let exp = app.notes.explorer.as_ref().unwrap().read(cx);
            assert_eq!(exp.active_buffer().unwrap().path, f.notes.join("misc/captured.md"));
            assert_eq!(exp.active_buffer().unwrap().original, "# Saved selection\n");
            assert_eq!(
                std::fs::read_to_string(f.dir.join("last-note")).unwrap().trim(),
                f.notes.join("misc").join("captured.md").display().to_string()
            );
        })
        .unwrap();
}

#[gpui_kit::test]
fn native_open_finishing_after_navigation_cannot_touch_project_tabs(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let path = f.notes.join("delayed.md");
    std::fs::write(&path, "# Delayed").unwrap();
    let window = f.window(cx);
    window
        .update(cx, |app, window, cx| {
            app.notes.last_note_file = Some(f.dir.join("last-note"));
            let project = app.active_project_explorer(cx);
            app.open_note(path.clone(), Some(NotesEditor::Aether), window, cx);
            app.page = Page::Chat;
            assert!(project.read(cx).tabs.is_empty());
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |app, _, cx| {
            assert_eq!(app.page, Page::Chat);
            assert!(app.active_project_explorer(cx).read(cx).tabs.is_empty());
            assert!(!app.notes.busy);
        })
        .unwrap();
}

#[gpui_kit::test]
fn project_archive_lists_its_global_archive_subtree(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let store = Store::new(f.notes.clone()).scoped(Some("project")).unwrap();
    let path = store.create(NoteKind::Named, "old", None, "archived text").unwrap();
    let archived = store.archive(&path).unwrap();
    let window = f.window(cx);
    window
        .update(cx, |app, window, cx| {
            app.open_notes(window, cx);
            app.notes.project = Some("project".into());
            app.notes.listing = Listing::Archive;
            app.refresh_notes(cx);
        })
        .unwrap();
    cx.background_executor.advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    window
        .update(cx, |app, _, _| {
            assert!(app.notes.entries.iter().any(|(path, _)| path == &archived));
        })
        .unwrap();
}

#[gpui_kit::test]
fn external_notes_reuse_the_global_terminal(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let window = f.window(cx);
    window
        .update(cx, |app, window, cx| {
            let global = app.ensure_global_terminal(window, cx);
            app.show_notes_terminal("true".into(), "fixture-connection".into(), window, cx);
            assert_eq!(app.page, Page::Terminal);
            assert_eq!(app.active_terminal_tab, aether_terminal::TerminalTab::Global);
            assert_eq!(app.global_terminal.as_ref().unwrap().view.entity_id(), global.entity_id());
            assert!(global.read(cx).command_running("notes", cx));
            app.page = Page::Notes;
            app.show_notes_terminal("true".into(), "fixture-connection".into(), window, cx);
            assert_eq!(app.page, Page::Terminal);
            assert_eq!(app.global_terminal.as_ref().unwrap().view.entity_id(), global.entity_id());
            assert!(global.read(cx).command_running("notes", cx));
            app.shutdown_terminals(cx);
        })
        .unwrap();
}

#[gpui_kit::test]
fn global_notes_search_previews_files_and_grep_without_navigating(cx: &mut TestAppContext) {
    cx.update(crate::actions::bind_keys);
    let f = Fixture::new();
    let file = f.notes.join("alpha.md");
    std::fs::write(&file, "# Alpha\n\nneedle match\n").unwrap();
    let window = f.window(cx);
    let app = window.root(cx).unwrap();
    window
        .update(cx, |app, window, cx| {
            app.page = Page::Settings;
            app.toggle_notes_search(window, cx);
            app.notes.search.update(cx, |input, cx| input.set_value("alpha", window, cx));
            app.queue_notes_search(cx);
        })
        .unwrap();
    cx.run_until_parked();
    cx.background_executor.advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    window
        .update(cx, |app, _, _| {
            assert_eq!(app.page, Page::Settings);
            assert_eq!(app.notes.search_ui.results.len(), 1);
            assert!(app.notes.search_ui.preview.as_ref().unwrap().markdown);
            assert!(app.notes.explorer.is_none(), "preview must not create editor buffers");
        })
        .unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("notes-search-modal").is_some());
    assert!(visual.debug_bounds("notes-search-preview").is_some());
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.notes.search.update(cx, |input, cx| input.set_value("/s needle", window, cx));
            app.queue_notes_search(cx);
        })
    });
    drop(visual);
    cx.run_until_parked();
    cx.background_executor.advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    window
        .update(cx, |app, window, cx| {
            assert_eq!(app.page, Page::Settings);
            assert_eq!(app.notes.search_ui.results[0].line, Some(3));
            let preview = app.notes.search_ui.preview.as_ref().unwrap();
            assert_eq!(preview.matched_line, Some(3));
            assert!(preview.text.contains("needle match"));
            app.notes.last_note_file = Some(f.dir.join("last-note"));
            app.open_note_result(window, cx);
            assert!(!app.notes.search_ui.open);
            assert_eq!(app.page, Page::Notes);
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |app, window, cx| {
            assert!(!app.notes.busy);
            assert_eq!(
                app.notes.explorer.as_ref().unwrap().read(cx).active_buffer().unwrap().path,
                file
            );
            let editor = app.active_explorer_editor(window, cx).unwrap();
            assert_eq!(editor.read(cx).cursor_position().line, 2);
        })
        .unwrap();
}

#[gpui_kit::test]
fn notes_search_global_shortcut_and_escape(cx: &mut TestAppContext) {
    let f = Fixture::new();
    cx.update(crate::actions::bind_keys);
    let window = f.window(cx);
    window.update(cx, |app, _, _| app.page = Page::Settings).unwrap();
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    visual.simulate_keystrokes("secondary-shift-n");
    visual.read(|cx| {
        assert!(app.read(cx).notes.search_ui.open);
        assert_eq!(app.read(cx).page, Page::Settings);
    });
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    visual.simulate_keystrokes("escape");
    visual.read(|cx| {
        assert!(!app.read(cx).notes.search_ui.open);
        assert_eq!(app.read(cx).page, Page::Settings);
    });
}

#[gpui_kit::test]
fn markdown_web_links_open_the_builtin_browser_from_notes(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let window = f.window(cx);
    window.update(cx, |app, _, _| app.page = Page::Notes).unwrap();
    let app = window.root(cx).unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    visual.update(|window, cx| {
        let open = cx.global::<aether_ui::OpenWebLink>().0.clone();
        assert!(open("https://example.com/notes#section", window, cx));
        assert!(!open("mailto:person@example.com", window, cx));
    });
    visual.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.page, Page::Chat);
        assert!(app.browser_open);
        assert_eq!(app.side_tab, crate::side_panel::SideTab::Browser);
        assert!(app.git.open.contains(&app.chats[app.active].id));
    });
}

#[gpui_kit::test]
fn notes_search_rows_fill_the_pane_without_gaps_or_toolbar_overlap(cx: &mut TestAppContext) {
    let f = Fixture::new();
    let window = f.window(cx);
    let app = window.root(cx).unwrap();
    window
        .update(cx, |app, _, _| {
            app.notes.search_ui.open = true;
            app.notes.search_ui.results = [
                "daily/today.md",
                "projects/a-very-long-project-name/notes/another-long-file-name.md",
            ]
            .into_iter()
            .map(|label| search::Hit {
                path: f.notes.join(label),
                label: label.into(),
                line: None,
                snippet: String::new(),
            })
            .collect();
        })
        .unwrap();
    let mut visual = gpui_kit::VisualTestContext::from_window(*window, cx);
    for mode in [search::Mode::Files, search::Mode::Grep] {
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.notes.search_ui.mode = mode;
                for hit in &mut app.notes.search_ui.results {
                    hit.line = (mode == search::Mode::Grep).then_some(12);
                    hit.snippet = "matching line context".into();
                }
                cx.notify();
            });
            let _ = window.draw(cx);
        });
        let pane = visual.debug_bounds("notes-search-results-pane").unwrap();
        let toolbar = visual.debug_bounds("notes-search-toolbar").unwrap();
        let first = visual.debug_bounds("notes-search-result-0").unwrap();
        let second = visual.debug_bounds("notes-search-result-1").unwrap();
        let preview = visual.debug_bounds("notes-search-preview").unwrap();
        assert!((first.size.width - pane.size.width).abs() < gpui_kit::px(1.));
        assert!((second.size.width - pane.size.width).abs() < gpui_kit::px(1.));
        assert!((second.top() - first.bottom()).abs() < gpui_kit::px(1.));
        assert!(first.top() >= toolbar.bottom());
        assert!((pane.right() - preview.left()).abs() < gpui_kit::px(1.));
        assert_eq!(
            first.size.height,
            gpui_kit::px(if mode == search::Mode::Grep { 56. } else { 36. })
        );
    }
}

#[gpui_kit::test]
fn notes_search_result_clicks_do_not_select_preview_text(cx: &mut TestAppContext) {
    use gpui_kit::base::TextSelection;
    use gpui_kit::{Modifiers, MouseButton, point, px};
    let f = Fixture::new();
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let mut settings = Settings {
        provider: Some(Provider::OpenRouter),
        workspace_root: f.project.display().to_string(),
        ..Settings::default()
    };
    settings.notes.directory = f.notes.display().to_string();
    let (window, app) = cx.update(|cx| {
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                ChatApp::with_state(
                    aether_agent::Agent::new().unwrap(),
                    settings,
                    None,
                    None,
                    false,
                    window,
                    cx,
                )
            })
        })
        .unwrap()
    });
    cx.update(|cx| app.update(cx, |app, _| {
        app.notes.search_ui.open = true;
        app.notes.search_ui.results = vec![search::Hit { path: f.notes.join("preview.md"), label: "preview.md".into(), line: None, snippet: String::new() }];
        app.notes.search_ui.preview = Some(search::Preview {
            text: "This preview has selectable words for copying.\n\nAnother paragraph in the preview.".into(),
            first_line: 1, matched_line: None, markdown: true, truncated: false,
        });
    }));
    let mut visual = gpui_kit::VisualTestContext::from_window(window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let row = visual.debug_bounds("notes-search-result-0").unwrap();
    let start = point(row.left() + px(30.), row.center().y);
    let preview = visual.debug_bounds("notes-search-preview").unwrap();
    let end = point(preview.left() + px(180.), preview.top() + px(60.));
    visual.simulate_mouse_move(start, None, Modifiers::default());
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    visual.update(|window, cx| {
        assert!(
            !TextSelection::has_selection(window, cx),
            "file-row gestures must not select preview text"
        );
    });
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    // The preview itself remains selectable for copying.
    let text_start = point(preview.left() + px(24.), preview.top() + px(60.));
    visual.simulate_mouse_move(text_start, None, Modifiers::default());
    visual.simulate_mouse_down(text_start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    visual.update(|window, cx| {
        assert!(TextSelection::has_selection(window, cx), "preview text must still be selectable");
    });
}
