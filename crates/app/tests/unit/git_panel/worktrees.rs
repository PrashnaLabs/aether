use super::*;

fn tree(path: &str, branch: Option<&str>) -> Listed {
    Listed {
        path: path.into(),
        repo: "/code/app".into(),
        branch: branch.map(String::from),
        head: "a1b2c3d4e5f6".into(),
        ..Listed::default()
    }
}

#[test]
fn rows_name_the_folder_branch_and_state() {
    let main = Listed { main: true, dirty: Some(false), ..tree("/code/app", Some("main")) };
    assert_eq!(tree_key(&main), "worktree-/code/app");
    assert_eq!((tree_name(&main), tree_branch(&main)), ("app".into(), "main".into()));
    assert_eq!(tree_flags(&main, true), ["current"]);
    assert_eq!(
        tree_label(&main, true),
        "app, on main, main worktree, current. Press Enter to open it in a new chat"
    );

    let detached = Listed { dirty: Some(true), ..tree("/w/wt ünï", None) };
    assert_eq!(tree_branch(&detached), "detached a1b2c3d");
    assert_eq!(tree_flags(&detached, false), ["changes"]);
    assert_eq!(
        tree_label(&detached, false),
        "wt ünï, on detached a1b2c3d, uncommitted changes. \
         Press Enter to open it in a new chat, Delete to remove it"
    );

    let locked = Listed { locked: Some("usb".into()), ..tree("/w/usb", Some("usb")) };
    assert_eq!(tree_flags(&locked, false), ["locked"]);
    assert_eq!(
        tree_label(&locked, false),
        "usb, on usb, locked: usb. Press Enter to open it in a new chat"
    );
    let gone = Listed { missing: true, ..tree("/w/gone", Some("gone")) };
    assert_eq!(tree_flags(&gone, false), ["missing"]);
    assert_eq!(tree_label(&gone, false), "gone, on gone, folder missing. Press Delete to prune it");
}

#[test]
fn branches_checked_out_elsewhere_say_where() {
    let trees = [tree("/code/app", Some("main")), tree("/w/review", Some("feature/x"))];
    let top = Path::new("/code/app");
    assert_eq!(checked_out_at(&trees, top, "main").as_deref(), Some("here"));
    assert_eq!(checked_out_at(&trees, top, "feature/x").as_deref(), Some("in review"));
    assert_eq!(checked_out_at(&trees, top, "free"), None);
    assert_eq!(checked_out_at(&trees, Path::new("/w/review"), "main").as_deref(), Some("in app"));
}

#[test]
fn worktree_picker_titles_say_what_enter_makes() {
    assert_eq!(tree_pick_title(&PickMode::Switch), "Create worktree");
    assert_eq!(tree_pick_title(&PickMode::Base), "Create worktree on a new branch from…");
    assert_eq!(tree_pick_title(&PickMode::Name(None)), "Create worktree on a new branch from HEAD");
    assert_eq!(
        tree_pick_title(&PickMode::Name(Some("refs/remotes/origin/main".into()))),
        "Create worktree on a new branch from origin/main"
    );
    let place = |start: Start| tree_pick_title(&PickMode::Place(start));
    assert_eq!(place(Start::Local("dev".into())), "Create worktree on dev in…");
    assert_eq!(place(Start::Remote("origin/api".into())), "Create worktree on origin/api in…");
    assert_eq!(
        place(Start::New { name: "fix/ünï".into(), base: None }),
        "Create worktree on fix/ünï in…"
    );
}

#[test]
fn menu_creates_once_there_is_a_commit_and_prunes_when_idle() {
    let born = Status { commit: Some("abc1234".into()), ..Status::default() };
    assert_eq!(tree_menu(&born, false, None), (true, true));
    assert_eq!(tree_menu(&Status::default(), false, None), (false, true));
    assert_eq!(tree_menu(&born, true, None), (false, false));
    assert_eq!(tree_menu(&born, false, Some(TreeJob::Prune)), (false, false));
}

#[test]
fn prune_says_what_goes_and_what_went() {
    assert!(prune_prompt(0).contains("None listed here are gone."));
    assert!(prune_prompt(1).contains("1 listed here is gone."));
    assert!(prune_prompt(3).contains("drive that isn't connected"));
    assert_eq!(prune_note(&[]), "Nothing to prune");
    assert_eq!(prune_note(&["a".into()]), "Pruned worktree a");
    assert_eq!(prune_note(&["a".into(), "b".into()]), "Pruned 2 worktrees");
}
