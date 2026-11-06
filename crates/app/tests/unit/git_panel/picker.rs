use super::*;

fn branches() -> Branches {
    Branches {
        current: Some("main".into()),
        local: vec!["feature/login".into(), "main".into(), "old".into()],
        remote: vec!["origin/main".into(), "origin/feature/api".into()],
    }
}

fn local(name: &str) -> PickRow {
    PickRow::Local(name.into())
}

fn remote(name: &str) -> PickRow {
    PickRow::Remote(name.into())
}

#[test]
fn switching_lists_create_actions_then_local_and_remote_branches() {
    assert_eq!(
        pick_rows(&branches(), &PickMode::Switch, ""),
        [
            PickRow::Create,
            PickRow::CreateFrom,
            PickRow::Caption("Branches"),
            local("feature/login"),
            local("main"),
            local("old"),
            PickRow::Caption("Remote branches"),
            remote("origin/main"),
            remote("origin/feature/api"),
        ]
    );
    assert_eq!(
        pick_rows(&branches(), &PickMode::Switch, " FEATURE "),
        [
            PickRow::Create,
            PickRow::CreateFrom,
            PickRow::Caption("Branches"),
            local("feature/login"),
            PickRow::Caption("Remote branches"),
            remote("origin/feature/api"),
        ]
    );
    assert_eq!(
        pick_rows(&branches(), &PickMode::Switch, "zzz"),
        [PickRow::Create, PickRow::CreateFrom]
    );
    assert_eq!(
        pick_rows(&Branches::default(), &PickMode::Switch, ""),
        [PickRow::Create, PickRow::CreateFrom]
    );
}

#[test]
fn choosing_a_base_lists_only_branches_and_naming_lists_nothing() {
    assert_eq!(
        pick_rows(&branches(), &PickMode::Base, "main"),
        [
            PickRow::Caption("Branches"),
            local("main"),
            PickRow::Caption("Remote branches"),
            remote("origin/main"),
        ]
    );
    assert!(pick_rows(&branches(), &PickMode::Base, "zzz").is_empty());
    assert!(pick_rows(&branches(), &PickMode::Name(None), "").is_empty());
    let place = PickMode::Place(Start::Local("old".into()));
    let folders = [PickRow::AetherFolder, PickRow::ChooseFolder];
    assert_eq!(pick_rows(&branches(), &place, "typed"), folders);
    assert_eq!(first_pick(&folders, "typed"), Some(0));
    assert_eq!(pick_step(&folders, Some(1), 1), Some(0));
}

#[test]
fn the_highlight_starts_on_a_typed_match_and_skips_captions() {
    let all = pick_rows(&branches(), &PickMode::Switch, "");
    assert_eq!(first_pick(&all, ""), Some(0));
    let typed = pick_rows(&branches(), &PickMode::Switch, "old");
    assert_eq!(typed[first_pick(&typed, "old").unwrap()], local("old"));
    let none = pick_rows(&branches(), &PickMode::Switch, "zzz");
    assert_eq!(first_pick(&none, "zzz"), Some(0));
    let base = pick_rows(&branches(), &PickMode::Base, "");
    assert_eq!(first_pick(&base, ""), Some(1));
    assert_eq!(first_pick(&[], "x"), None);

    // CreateFrom (1) steps over the "Branches" caption (2)
    assert_eq!(pick_step(&all, Some(1), 1), Some(3));
    assert_eq!(pick_step(&all, Some(3), -1), Some(1));
    assert_eq!(pick_step(&all, Some(5), 1), Some(7));
    assert_eq!(pick_step(&all, Some(8), 1), Some(0));
    assert_eq!(pick_step(&all, Some(0), -1), Some(8));
    assert_eq!(pick_step(&all, None, 1), Some(0));
    assert_eq!(pick_step(&base, None, 1), Some(1));
    assert_eq!(pick_step(&base, None, -1), Some(6));
    assert_eq!(pick_step(&[], None, 1), None);
    assert_eq!(pick_step(&[PickRow::Caption("x")], Some(0), 1), None);
}

#[test]
fn titles_say_where_a_new_branch_starts() {
    assert_eq!(pick_title(&PickMode::Switch), "Switch branch");
    assert_eq!(pick_title(&PickMode::Base), "Create branch from…");
    assert_eq!(pick_title(&PickMode::Name(None)), "Create branch from HEAD");
    assert_eq!(
        pick_title(&PickMode::Name(Some("refs/remotes/origin/main".into()))),
        "Create branch from origin/main"
    );
    assert_eq!(
        pick_title(&PickMode::Name(Some("refs/heads/feature/x".into()))),
        "Create branch from feature/x"
    );
}
