use super::*;

#[test]
fn maps_common_paths_to_semantic_icons() {
    assert_eq!(file_icon(Path::new("src/main.rs")).path(), IconName::FileCode.path());
    assert_eq!(file_icon(Path::new("Cargo.toml")).path(), IconName::Braces.path());
    assert_eq!(file_icon(Path::new("Cargo.lock")).path(), IconName::Lock.path());
    assert_eq!(file_icon(Path::new("README.md")).path(), IconName::BookOpen.path());
    assert_eq!(file_icon(Path::new(".gitignore")).path(), IconName::GitBranch.path());
    assert_eq!(file_icon(Path::new("scripts/run.sh")).path(), IconName::SquareTerminal.path());
    assert_eq!(file_icon(Path::new("assets/logo.svg")).path(), IconName::FileImage.path());
    assert_eq!(file_icon(Path::new("notes.txt")).path(), IconName::FileText.path());
}
