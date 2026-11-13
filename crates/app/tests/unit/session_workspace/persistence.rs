use super::*;
use crate::session_workspace::model::*;
use std::collections::HashSet;

#[test]
fn round_trip_keeps_floats_and_independent_drafts() {
    let mut saved = SavedWorkspace::default();
    let a = saved.model.open(11, 0);
    let b = saved.model.split(a, 22, SplitAxis::Horizontal, [1000., 600.]).unwrap();
    saved.model.float(b, Rect::new(40., 50., 400., 300.)).unwrap();
    let encoded = serde_json::to_string(&saved).unwrap();
    let loaded = decode_layout(&encoded, &HashSet::from([11, 22]), [1000., 700.]).unwrap();
    assert_eq!(loaded.windows[&0].floats[0].bounds, Rect::new(40., 50., 400., 300.));
    assert_eq!(loaded.panes.len(), 2);
    let mut drafts = SavedDrafts::default();
    drafts.drafts.insert(
        11,
        SessionSnapshot { text: "alpha".into(), selection: 1..3, ..Default::default() },
    );
    drafts.drafts.insert(
        22,
        SessionSnapshot {
            text: "beta".into(),
            editor: Some("editing beta".into()),
            ..Default::default()
        },
    );
    let decoded: SavedDrafts =
        serde_json::from_str(&serde_json::to_string(&drafts).unwrap()).unwrap();
    assert_eq!(decoded.drafts[&11].text, "alpha");
    assert_eq!(decoded.drafts[&22].editor.as_deref(), Some("editing beta"));
}

#[test]
fn repair_removes_missing_and_duplicate_chats_and_stale_focus() {
    let mut saved = SavedWorkspace::default();
    let a = saved.model.open(11, 0);
    let b = saved.model.open(22, 0);
    saved.model.panes.insert(b, 11);
    saved.model.windows.get_mut(&0).unwrap().focused = Some(999);
    let loaded =
        decode_layout(&serde_json::to_string(&saved).unwrap(), &HashSet::from([11]), [800., 600.])
            .unwrap();
    assert_eq!(loaded.panes.len(), 1);
    assert_eq!(loaded.windows[&0].panes(), vec![a]);
    assert_eq!(loaded.windows[&0].focused, Some(a));
}

#[test]
fn unsupported_or_truncated_layout_is_rejected() {
    assert!(decode_layout(r#"{"version":99}"#, &HashSet::new(), [800., 600.]).is_err());
    assert!(decode_layout("{", &HashSet::new(), [800., 600.]).is_err());
}

#[test]
fn failed_temp_write_keeps_previous_layout_file() {
    let dir = std::env::temp_dir().join(format!(
        "aether-layout-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("layout.json");
    std::fs::write(&path, b"previous").unwrap();
    std::fs::create_dir(path.with_extension("json.tmp")).unwrap();
    assert!(atomic_write(&path, b"replacement").is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous");
    std::fs::remove_dir(path.with_extension("json.tmp")).unwrap();
    atomic_write(&path, b"replacement").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unchanged_inputs_skip_serialization_but_layout_and_drafts_save() {
    let mut inputs = None;
    let mut model = WorkspaceModel::default();
    let mut drafts =
        BTreeMap::from([(11, SessionSnapshot { text: "draft".into(), ..Default::default() })]);
    assert!(SaveState::payload(&mut inputs, &model, drafts.clone()).is_some());
    assert!(SaveState::payload(&mut inputs, &model, drafts.clone()).is_none());

    model.windows.get_mut(&0).unwrap().shown = Some(99);
    assert!(
        SaveState::payload(&mut inputs, &model, drafts.clone()).is_none(),
        "transient layout fields must not trigger a save"
    );

    model.open(11, 0);
    let (layout, _) = SaveState::payload(&mut inputs, &model, drafts.clone()).unwrap();
    let saved: SavedWorkspace = serde_json::from_str(&layout).unwrap();
    assert_eq!(saved.model, model);
    drafts.get_mut(&11).unwrap().text.push_str(" edited");
    let (_, encoded) = SaveState::payload(&mut inputs, &model, drafts.clone()).unwrap();
    let saved: SavedDrafts = serde_json::from_str(&encoded).unwrap();
    assert_eq!(saved.drafts[&11].text, "draft edited");
    assert!(SaveState::payload(&mut inputs, &model, drafts).is_none());
}

#[gpui::test]
fn live_composer_edits_invalidate_saved_inputs(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::AppContext;
    cx.update(gpui_kit::init);
    cx.update(crate::actions::bind_keys);
    let window = cx.add_window(|window, cx| {
        ChatApp::with_state(
            aether_agent::Agent::new().unwrap(),
            aether_core::Settings::default(),
            None,
            None,
            false,
            window,
            cx,
        )
    });
    let app = window.root(cx).unwrap();
    cx.update_window(*window, |_, window, cx| {
        app.update(cx, |app, cx| {
            let mut inputs = None;
            let mut payload = |app: &ChatApp, cx: &App| {
                SaveState::payload(&mut inputs, &app.workspace.model, app.session_snapshots(cx))
            };
            assert!(payload(app, cx).is_some());
            assert!(payload(app, cx).is_none());
            let id = app.chats[app.active].id;
            app.activate_session_ui(id, window, cx);
            app.composer.update(cx, |composer, cx| {
                composer.set_value("edited composer draft", window, cx);
            });
            let (_, encoded) = payload(app, cx).unwrap();
            let saved: SavedDrafts = serde_json::from_str(&encoded).unwrap();
            assert_eq!(saved.drafts[&id].text, "edited composer draft");
            assert!(payload(app, cx).is_none());
        });
    })
    .unwrap();
}
