use super::*;
use crate::app::Page;
use crate::settings_view::SettingsTab;
use gpui_kit::component::WindowExt;
use gpui_kit::{AppContext, VisualTestContext};

#[gpui_kit::test]
fn selecting_and_deleting_files_requires_confirmation(cx: &mut gpui_kit::TestAppContext) {
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    cx.update(crate::actions::bind_keys);
    let dir = std::env::temp_dir().join(format!(
        "aether-attachment-selection-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let paths: Vec<_> = ["1-first.txt", "2-second.txt", "3-kept.txt"]
        .map(|name| {
            let path = dir.join(name);
            std::fs::write(&path, "attachment").unwrap();
            path
        })
        .into();
    let (window, app) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| {
                let mut app = ChatApp::with_state(
                    aether_agent::Agent::new().unwrap(),
                    aether_core::Settings::default(),
                    None,
                    None,
                    false,
                    window,
                    cx,
                );
                app.open_page(Page::Settings, cx);
                app.settings_tab = SettingsTab::Attachments;
                app.stored_files = paths
                    .iter()
                    .enumerate()
                    .map(|(i, path)| StoredFile {
                        chat: if i < 2 { app.chats[app.active].id } else { i64::MIN },
                        path: path.clone(),
                        size: 10,
                        modified: std::time::SystemTime::now(),
                    })
                    .collect();
                app
            })
        })
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window, cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(visual.debug_bounds("attachments-project-filter").is_some());
    assert!(visual.debug_bounds("stored-files-grid").is_some());
    assert!(visual.debug_bounds("stored-files-Removed chats").is_none());
    let project = visual.read(|cx| {
        let app = app.read(cx);
        app.stored_file_project(&app.stored_files[0]).unwrap()
    });
    for (filter, expected) in [
        (ProjectFilter::Project(project.clone()), 2),
        (ProjectFilter::RemovedChats, 1),
        (ProjectFilter::Project(dir.join("no-such-project")), 0),
        (ProjectFilter::All, 3),
    ] {
        visual.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.stored_files_project_filter = filter.clone();
                assert_eq!(app.visible_stored_files(None).len(), expected);
                assert_eq!(
                    app.visible_stored_files(Some(&project)).len(),
                    2,
                    "the dashboard scope ignores the settings filter"
                );
                cx.notify();
            });
            let _ = window.draw(cx);
        });
        assert!(
            visual.debug_bounds("attachments-project-filter").is_some(),
            "the filter remains available even when no files match"
        );
        assert_eq!(
            visual.debug_bounds("stored-file-1-first.txt").is_some(),
            matches!(filter, ProjectFilter::All)
                || matches!(&filter, ProjectFilter::Project(p) if p == &project)
        );
        assert_eq!(
            visual.debug_bounds("stored-file-3-kept.txt").is_some(),
            matches!(filter, ProjectFilter::All | ProjectFilter::RemovedChats)
        );
    }
    assert!(visual.debug_bounds("attachments-selection-bar").is_none());
    let first = visual.debug_bounds("stored-file-select-1-first.txt").unwrap();
    visual.simulate_mouse_move(first.center(), None, Default::default());
    visual.simulate_click(first.center(), Default::default());
    visual.read(|cx| assert_eq!(app.read(cx).stored_file_picks.len(), 1));
    assert!(visual.debug_bounds("attachments-selection-bar").is_some());
    let all = visual.debug_bounds("attachments-select-all").unwrap();
    visual.simulate_click(all.center(), Default::default());
    visual.read(|cx| assert_eq!(app.read(cx).stored_file_picks.len(), 3));
    let clear = visual.debug_bounds("attachments-clear-selection").unwrap();
    visual.simulate_click(clear.center(), Default::default());
    visual.read(|cx| assert!(app.read(cx).stored_file_picks.is_empty()));
    assert!(visual.debug_bounds("attachments-selection-bar").is_none());
    for selector in ["stored-file-select-1-first.txt", "stored-file-select-2-second.txt"] {
        let bounds = visual.debug_bounds(selector).unwrap();
        visual.simulate_mouse_move(bounds.center(), None, Default::default());
        visual.simulate_click(bounds.center(), Default::default());
    }
    visual.read(|cx| assert_eq!(app.read(cx).stored_file_picks.len(), 2));
    let delete = visual.debug_bounds("attachments-delete-selected").unwrap();
    visual.simulate_click(delete.center(), Default::default());
    visual.update(|window, cx| assert!(window.has_active_dialog(cx)));
    assert!(paths.iter().all(|path| path.exists()), "opening the dialog must not delete files");
    visual.simulate_keystrokes("escape");
    assert!(paths.iter().all(|path| path.exists()), "Cancel preserves every file");
    visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
    let delete = visual.debug_bounds("attachments-delete-selected").unwrap();
    visual.simulate_click(delete.center(), Default::default());
    visual.simulate_keystrokes("enter");
    assert!(!paths[0].exists());
    assert!(!paths[1].exists());
    assert!(paths[2].exists(), "unselected files stay on disk");
    visual.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.stored_files.len(), 1);
        assert!(app.stored_file_picks.is_empty());
        assert!(app.missing_attachments.contains(&paths[0]));
    });
    assert!(visual.debug_bounds("attachments-selection-bar").is_none());
    // Single-file deletion shares the same confirmation path.
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            app.confirm_delete_stored_files(vec![paths[2].clone()], window, cx);
        })
    });
    visual.update(|window, cx| assert!(window.has_active_dialog(cx)));
    assert!(paths[2].exists());
    visual.simulate_keystrokes("escape");
    assert!(paths[2].exists());
    std::fs::remove_dir_all(dir).unwrap();
}
