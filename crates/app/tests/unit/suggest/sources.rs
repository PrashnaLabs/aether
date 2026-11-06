use super::*;

#[test]
fn terminal_commands_detected_dynamically() {
    let temp = std::env::temp_dir().join(format!(
        "aether_cmd_test_{}",
        std::time::SystemTime::now().elapsed().unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&temp).unwrap();
    std::fs::write(temp.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();

    let cmds = detect_terminal_commands(Some(&temp), "");
    assert!(cmds.iter().any(|(c, _)| c == "cargo test"));
    assert!(cmds.iter().any(|(c, _)| c == "cargo check"));

    let filtered = detect_terminal_commands(Some(&temp), "clippy");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].0, "cargo clippy");

    let _ = std::fs::remove_dir_all(&temp);
}
