use super::*;

#[test]
fn claude_report_gives_exact_parts_and_live_servers() {
    let ctx = json!({
        "categories": [
            {"id": "system_prompt", "name": "System prompt", "tokens": 2502, "kind": "used"},
            {"id": "mcp_tools", "name": "MCP tools", "tokens": 647, "kind": "used"},
            {"id": "mcp_tools_deferred", "name": "MCP tools (deferred)", "tokens": 9000, "kind": "deferred"},
            {"id": "messages", "name": "Messages", "tokens": 4755, "kind": "used"},
            {"id": "autocompact_buffer", "name": "Autocompact buffer", "tokens": 33000, "kind": "buffer"},
            {"id": "free_space", "name": "Free space", "tokens": 1, "kind": "free"}
        ],
        "memoryFiles": [{"path": "/h/.claude/CLAUDE.md", "type": "User", "tokens": 2374}],
        "agents": [{"agentType": "reviewer", "source": "user", "path": "/h/a.md", "tokens": 27}],
        "skills": {"skillFrontmatter": [{"name": "tdd", "path": "/h/tdd/SKILL.md", "tokens": 67}]},
        "mcpTools": [
            {"name": "mcp__claude_ai_Figma__a", "serverName": "claude_ai_Figma", "tokens": 100, "isLoaded": true},
            {"name": "mcp__claude_ai_Figma__b", "serverName": "claude_ai_Figma", "tokens": 50, "isLoaded": false}
        ]
    });
    let mcp = json!({"mcpServers": [
        {"name": "claude.ai Figma", "status": "connected"},
        {"name": "claude.ai Canva", "status": "needs-auth"},
        {"name": "postman", "status": "failed"}
    ]});
    let disk =
        vec![Item { name: "postman".into(), path: "/p/.mcp.json".into(), ..Item::default() }];
    let inv = claude_inventory(&ctx, &mcp, disk);
    assert_eq!(inv.parts, [("System prompt".into(), 2502), ("MCP tools".into(), 647)]);
    assert_eq!(inv.buffer, 33000);
    assert!(inv.exact());
    assert_eq!(inv.memory[0].tokens, 2374);
    assert_eq!((inv.agents[0].name.as_str(), inv.skills[0].tokens), ("reviewer", 67));
    let servers: Vec<_> = inv.mcp.iter().map(|s| (s.name.as_str(), s.status, s.tokens)).collect();
    assert_eq!(
        servers,
        [
            ("claude.ai Figma", Some(Status::Connected), 100),
            ("claude.ai Canva", Some(Status::NeedsAuth), 0),
            ("postman", Some(Status::Failed), 0)
        ]
    );
    assert_eq!(inv.mcp[2].path, Path::new("/p/.mcp.json"));
}

#[test]
fn codex_servers_read_auth_and_schema_size() {
    let list = json!({"data": [
        {"name": "figma", "authStatus": "notLoggedIn", "tools": {}},
        {"name": "node_repl", "authStatus": "unsupported", "tools": {"run": {"inputSchema": {"type": "object"}}}},
        {"name": "broken", "authStatus": "unsupported", "toolsError": "spawn failed"}
    ]});
    let servers = codex_servers(&list);
    assert_eq!(servers[0].status, Some(Status::NeedsAuth));
    assert_eq!(servers[1].status, Some(Status::Connected));
    assert!(servers[1].tokens > 0);
    assert_eq!(servers[2].status, Some(Status::Failed));
}

#[test]
fn opencode_lists_servers_and_offered_subagents() {
    let mcp = json!({"data": [{"name": "docs", "status": {"status": "needs_auth", "error": "x"}}]});
    assert_eq!(opencode_servers(&mcp)[0].status, Some(Status::NeedsAuth));
    let agents = json!({"data": [
        {"id": "build", "mode": "primary", "hidden": false, "description": "Default"},
        {"id": "explore", "mode": "subagent", "hidden": false, "description": "Reads code"},
        {"id": "title", "mode": "primary", "hidden": true, "description": ""}
    ]});
    let names: Vec<String> = opencode_agents(&agents).into_iter().map(|a| a.name).collect();
    assert_eq!(names, ["explore"]);
}

#[test]
#[ignore = "runs the installed claude and codex CLIs"]
fn probes_installed_clis() {
    let cwd = std::env::temp_dir();
    for provider in [Provider::Claude, Provider::Codex] {
        let mut inv = Inventory::default();
        probe(provider, &Settings::default(), &cwd, &mut inv);
        println!("{provider:?}: {inv:#?}");
        assert!(!inv.mcp.is_empty() || inv.exact(), "{provider:?} gave nothing");
    }
}
