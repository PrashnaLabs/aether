use super::*;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn reads_toml_mcp_tables_once_each() {
    let text = "[mcp_servers.figma]\ncommand = \"x\"\n[mcp_servers.figma.env]\nA = \"1\"\n[mcp_servers.\"my.db\"]\n[other]\n";
    assert_eq!(toml_tables(text, "mcp_servers"), ["figma", "my.db"]);
}

#[test]
fn scans_claude_home_and_project() {
    let root = std::env::temp_dir().join(format!("aether-inventory-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let home = root.join("home");
    let repo = root.join("repo");
    let cwd = repo.join("sub");
    fs::create_dir_all(repo.join(".git")).unwrap();
    write(&home.join(".claude/CLAUDE.md"), "12345678");
    write(&repo.join("CLAUDE.md"), "1234");
    write(&cwd.join("CLAUDE.md"), "1234");
    write(&root.join("CLAUDE.md"), "above the repo, ignored");
    let projects = format!(
        r#"{{"mcpServers":{{"postman":{{}}}},"projects":{{"{}":{{"mcpServers":{{"supabase":{{}}}}}}}}}}"#,
        cwd.display()
    );
    write(&home.join(".claude.json"), &projects);
    write(&repo.join(".mcp.json"), r#"{"mcpServers":{"postman":{},"figma":{}}}"#);
    write(
        &home.join(".claude/agents/reviewer.md"),
        "---\nname: reviewer\ndescription: Reviews diffs.\n---\nBody",
    );
    write(&repo.join(".claude/agents/notes.txt"), "not an agent");
    let skill = Skill {
        name: "tdd".into(),
        description: "Test first.".into(),
        instructions: String::new(),
        source: SkillSource::ClaudeCode,
        dir: home.join(".claude/skills/tdd"),
    };
    let codex_skill = Skill { source: SkillSource::Codex, ..skill.clone() };

    let inv = scan(Provider::Claude, Some(&cwd), &home, &[skill, codex_skill]);

    let memory: Vec<&str> = inv.memory.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(memory.len(), 3, "{memory:?}");
    assert_eq!(memory[0], "~/.claude/CLAUDE.md");
    assert_eq!(inv.memory[0].tokens, 2);
    let mcp: Vec<&str> = inv.mcp.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(mcp, ["postman", "supabase", "figma"]);
    assert_eq!(inv.agents.len(), 1);
    assert_eq!(inv.agents[0].name, "reviewer");
    assert_eq!(inv.skills.len(), 1);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn open_router_lists_only_aether_skills() {
    let inv = scan(Provider::OpenRouter, None, Path::new("/nonexistent"), &[]);
    assert_eq!(inv, Inventory::default());
}
