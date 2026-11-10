use std::fs;
use std::path::{Path, PathBuf};

use aether_core::{Provider, Skill, SkillSource};
use serde_json::Value;

/// One thing a provider loads into its context: a memory file, MCP server, agent or skill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub path: PathBuf,
    /// Rough token cost, bytes / 4; 0 when unknown.
    pub tokens: u64,
}

/// What a provider loads for a session in `cwd`, read from its config files on disk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    pub memory: Vec<Item>,
    pub mcp: Vec<Item>,
    pub agents: Vec<Item>,
    pub skills: Vec<Item>,
}

struct Layout {
    memory_home: &'static [&'static str],
    memory_project: &'static [&'static str],
    mcp_home: &'static [(&'static str, &'static str)],
    mcp_project: &'static [(&'static str, &'static str)],
    agents_home: &'static [&'static str],
    agents_project: &'static [&'static str],
    skills: &'static [SkillSource],
}

fn layout(provider: Provider) -> Option<Layout> {
    Some(match provider {
        Provider::Claude => Layout {
            memory_home: &[".claude/CLAUDE.md"],
            memory_project: &["CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md"],
            mcp_home: &[(".claude.json", "mcpServers")],
            mcp_project: &[(".mcp.json", "mcpServers")],
            agents_home: &[".claude/agents"],
            agents_project: &[".claude/agents"],
            skills: &[SkillSource::ClaudeCode],
        },
        Provider::Codex => Layout {
            memory_home: &[".codex/AGENTS.md"],
            memory_project: &["AGENTS.md"],
            mcp_home: &[(".codex/config.toml", "mcp_servers")],
            mcp_project: &[(".codex/config.toml", "mcp_servers")],
            agents_home: &[".codex/agents"],
            agents_project: &[".codex/agents"],
            skills: &[SkillSource::Codex, SkillSource::Agents],
        },
        Provider::Pi => Layout {
            memory_home: &[".pi/agent/AGENTS.md"],
            memory_project: &["AGENTS.md", "CLAUDE.md"],
            mcp_home: &[(".pi/agent/mcp.json", "mcpServers")],
            mcp_project: &[(".pi/mcp.json", "mcpServers")],
            agents_home: &[".pi/agent/agents"],
            agents_project: &[".pi/agents"],
            skills: &[SkillSource::Agents],
        },
        Provider::OpenCode => Layout {
            memory_home: &[".config/opencode/AGENTS.md"],
            memory_project: &["AGENTS.md"],
            mcp_home: &[(".config/opencode/opencode.json", "mcp")],
            mcp_project: &[("opencode.json", "mcp")],
            agents_home: &[".config/opencode/agent", ".config/opencode/agents"],
            agents_project: &[".opencode/agent", ".opencode/agents"],
            skills: &[SkillSource::OpenCode, SkillSource::ClaudeCode, SkillSource::Agents],
        },
        Provider::Antigravity => Layout {
            memory_home: &[".gemini/GEMINI.md"],
            memory_project: &["GEMINI.md"],
            mcp_home: &[
                (".gemini/config/mcp_config.json", "mcpServers"),
                (".gemini/settings.json", "mcpServers"),
            ],
            mcp_project: &[(".gemini/settings.json", "mcpServers")],
            agents_home: &[".gemini/agents"],
            agents_project: &[".gemini/agents"],
            skills: &[SkillSource::Antigravity],
        },
        Provider::OpenRouter => return None,
    })
}

/// Folders from the repo root (nearest `.git` above `cwd`, else `cwd`) down to `cwd`.
fn project_dirs(cwd: &Path) -> Vec<&Path> {
    let root = cwd.ancestors().find(|d| d.join(".git").exists()).unwrap_or(cwd);
    let mut dirs: Vec<&Path> = cwd.ancestors().take_while(|d| *d != root).collect();
    dirs.push(root);
    dirs.reverse();
    dirs
}

/// Claude Code's per-project folder name: every non-alphanumeric char becomes `-`.
fn claude_slug(cwd: &Path) -> String {
    cwd.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

fn file_item(path: PathBuf) -> Option<Item> {
    let len = fs::metadata(&path).ok().filter(|m| m.is_file())?.len();
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(Item { name, path, tokens: len / 4 })
}

/// Server names under `key` in a JSON or TOML config; `projects.<cwd>` too for `~/.claude.json`.
fn mcp_names(path: &Path, key: &str, cwd: Option<&Path>) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else { return vec![] };
    if path.extension().is_some_and(|e| e == "toml") {
        return toml_tables(&text, key);
    }
    let Ok(json) = serde_json::from_str::<Value>(&text) else { return vec![] };
    let keys = |v: &Value| v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    let mut names: Vec<String> = keys(&json[key]);
    if let Some(cwd) = cwd {
        names.extend(keys(&json["projects"][cwd.to_string_lossy().as_ref()][key]));
    }
    names
}

/// Names of `[key.NAME]` tables; nested `[key.NAME.env]` tables count once.
fn toml_tables(text: &str, key: &str) -> Vec<String> {
    let prefix = format!("[{key}.");
    let mut names: Vec<String> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix(&prefix)?.strip_suffix(']').map(str::to_owned))
        .map(|rest| match rest.strip_prefix('"') {
            Some(quoted) => quoted.split('"').next().unwrap_or_default().to_string(),
            None => rest.split('.').next().unwrap_or_default().to_string(),
        })
        .filter(|name| !name.is_empty())
        .collect();
    names.dedup();
    names
}

fn agent_item(path: PathBuf) -> Option<Item> {
    let ext = path.extension()?.to_str()?;
    if ext != "md" && ext != "toml" {
        return None;
    }
    let text = fs::read_to_string(&path).ok()?;
    let field = |key: &str| {
        text.lines().find_map(|l| {
            let v = l.trim().strip_prefix(key)?.trim_start().strip_prefix([':', '='])?;
            Some(v.trim().trim_matches('"').to_string())
        })
    };
    let name = field("name").unwrap_or_else(|| path.file_stem().unwrap().to_string_lossy().into());
    let description = field("description").unwrap_or_default();
    let tokens = (name.len() + description.len()) as u64 / 4;
    Some(Item { name, path, tokens })
}

fn push_unique(items: &mut Vec<Item>, item: Item) {
    if !items.iter().any(|i| i.name == item.name) {
        items.push(item);
    }
}

/// Reads what `provider` loads for a session in `cwd`: memory files, MCP servers, custom
/// agents and skills. Token counts are estimates; MCP tool schemas aren't counted.
pub fn scan(provider: Provider, cwd: Option<&Path>, home: &Path, skills: &[Skill]) -> Inventory {
    let Some(l) = layout(provider) else {
        return Inventory {
            skills: skill_items(skills, &[SkillSource::Aether]),
            ..Inventory::default()
        };
    };
    let dirs = cwd.map(project_dirs).unwrap_or_default();
    let mut inv = Inventory::default();

    let home_memory = l.memory_home.iter().map(|f| home.join(f));
    let project_memory = dirs.iter().flat_map(|d| l.memory_project.iter().map(|f| d.join(f)));
    let auto_memory = (provider == Provider::Claude)
        .then(|| {
            cwd.map(|c| home.join(".claude/projects").join(claude_slug(c)).join("memory/MEMORY.md"))
        })
        .flatten();
    for path in home_memory.chain(project_memory).chain(auto_memory) {
        if let Some(mut item) = file_item(path) {
            item.name = item.path.strip_prefix(home).map_or_else(
                |_| item.path.to_string_lossy().into_owned(),
                |rel| format!("~/{}", rel.to_string_lossy()),
            );
            inv.memory.push(item);
        }
    }

    let home_mcp = l.mcp_home.iter().map(|(f, k)| (home.join(f), *k));
    let project_mcp =
        dirs.iter().flat_map(|d| l.mcp_project.iter().map(move |(f, k)| (d.join(f), *k)));
    for (path, key) in home_mcp.chain(project_mcp) {
        for name in mcp_names(&path, key, cwd) {
            push_unique(&mut inv.mcp, Item { name, path: path.clone(), tokens: 0 });
        }
    }

    let home_agents = l.agents_home.iter().map(|d| home.join(d));
    let project_agents = dirs.iter().flat_map(|d| l.agents_project.iter().map(|a| d.join(a)));
    for dir in home_agents.chain(project_agents) {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        let mut found: Vec<Item> =
            entries.filter_map(Result::ok).filter_map(|e| agent_item(e.path())).collect();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        for item in found {
            push_unique(&mut inv.agents, item);
        }
    }

    inv.skills = skill_items(skills, l.skills);
    inv
}

fn skill_items(skills: &[Skill], sources: &[SkillSource]) -> Vec<Item> {
    skills
        .iter()
        .filter(|s| sources.contains(&s.source))
        .map(|s| Item {
            name: s.name.clone(),
            path: s.dir.clone(),
            tokens: (s.name.len() + s.description.len()) as u64 / 4,
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/inventory.rs"]
mod tests;
