use std::fs;
use std::path::{Path, PathBuf};

use aether_core::{Provider, Skill, SkillSource};
use serde_json::Value;

/// One thing a provider loads into its context: a memory file, MCP server, agent or skill.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub path: PathBuf,
    /// Token cost: exact when the CLI reported it, else bytes / 4; 0 when unknown.
    pub tokens: u64,
    /// MCP server state, when known.
    pub status: Option<Status>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connected,
    Pending,
    NeedsAuth,
    Failed,
    Disabled,
}

/// What a provider loads for a session in `cwd`: read from its config files on disk, then
/// replaced by what the CLI itself reports where it can (see `probe`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    pub memory: Vec<Item>,
    pub mcp: Vec<Item>,
    pub agents: Vec<Item>,
    pub skills: Vec<Item>,
    /// Exact fixed context parts the CLI reported (system prompt, tools, …), messages excluded.
    pub parts: Vec<(String, u64)>,
    /// Space the CLI keeps free for auto-compaction.
    pub buffer: u64,
}

impl Inventory {
    /// True once the CLI reported its own numbers.
    pub fn exact(&self) -> bool {
        !self.parts.is_empty()
    }
}

struct Layout {
    /// The provider's user config folder.
    base: PathBuf,
    /// Memory files under `base`; in each group the first existing file wins.
    memory_home: &'static [&'static [&'static str]],
    /// Memory files in each project folder, same grouping.
    memory_project: &'static [&'static [&'static str]],
    /// Project memory is read from every parent folder, not just up to the repo root.
    to_root: bool,
    mcp_home: &'static [(&'static str, &'static str)],
    mcp_project: &'static [(&'static str, &'static str)],
    agents_home: &'static [&'static str],
    agents_project: &'static [&'static str],
    skills: &'static [SkillSource],
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn layout(provider: Provider, home: &Path) -> Option<Layout> {
    Some(match provider {
        Provider::Claude => Layout {
            base: home.to_path_buf(),
            memory_home: &[&[".claude/CLAUDE.md"]],
            memory_project: &[&["CLAUDE.md"], &[".claude/CLAUDE.md"], &["CLAUDE.local.md"]],
            to_root: true,
            mcp_home: &[(".claude.json", "mcpServers")],
            mcp_project: &[(".mcp.json", "mcpServers")],
            agents_home: &[".claude/agents"],
            agents_project: &[".claude/agents"],
            skills: &[SkillSource::ClaudeCode],
        },
        Provider::Codex => Layout {
            base: env_dir("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            memory_home: &[&["AGENTS.override.md", "AGENTS.md"]],
            memory_project: &[&["AGENTS.override.md", "AGENTS.md"]],
            to_root: false,
            mcp_home: &[("config.toml", "mcp_servers")],
            mcp_project: &[(".codex/config.toml", "mcp_servers")],
            agents_home: &["agents"],
            agents_project: &[".codex/agents"],
            skills: &[SkillSource::Codex, SkillSource::Agents],
        },
        Provider::Pi => Layout {
            base: env_dir("PI_CODING_AGENT_DIR").unwrap_or_else(|| home.join(".pi/agent")),
            memory_home: &[&["AGENTS.override.md", "AGENTS.md", "CLAUDE.md"]],
            memory_project: &[&["AGENTS.override.md", "AGENTS.md", "CLAUDE.md"]],
            to_root: true,
            mcp_home: &[("mcp.json", "mcpServers")],
            mcp_project: &[(".pi/mcp.json", "mcpServers")],
            agents_home: &["agents"],
            agents_project: &[".pi/agents"],
            skills: &[SkillSource::Agents],
        },
        Provider::OpenCode => Layout {
            base: home.join(".config/opencode"),
            memory_home: &[&["AGENTS.md"]],
            memory_project: &[&["AGENTS.md", "CLAUDE.md"]],
            to_root: false,
            mcp_home: &[("opencode.json", "mcp"), ("opencode.jsonc", "mcp")],
            mcp_project: &[("opencode.json", "mcp"), ("opencode.jsonc", "mcp")],
            agents_home: &["agent", "agents"],
            agents_project: &[".opencode/agent", ".opencode/agents"],
            skills: &[SkillSource::OpenCode, SkillSource::ClaudeCode, SkillSource::Agents],
        },
        Provider::Antigravity => Layout {
            base: home.join(".gemini"),
            memory_home: &[&["GEMINI.md"]],
            memory_project: &[&["AGENTS.md"], &["GEMINI.md"]],
            to_root: false,
            mcp_home: &[("config/mcp_config.json", "mcpServers")],
            mcp_project: &[],
            agents_home: &["config/agents"],
            agents_project: &[".agents/agents"],
            skills: &[SkillSource::Antigravity],
        },
        Provider::OpenRouter => return None,
    })
}

/// Folders from the top (filesystem root when `to_root`, else the nearest `.git` above `cwd`,
/// else `cwd`) down to `cwd`.
fn project_dirs(cwd: &Path, to_root: bool) -> Vec<&Path> {
    let mut dirs: Vec<&Path> = if to_root {
        cwd.ancestors().collect()
    } else {
        let root = cwd.ancestors().find(|d| d.join(".git").exists()).unwrap_or(cwd);
        let mut dirs: Vec<&Path> = cwd.ancestors().take_while(|d| *d != root).collect();
        dirs.push(root);
        dirs
    };
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
    Some(Item { name, path, tokens: len / 4, status: None })
}

/// The first existing file of each group, in `dir`.
fn first_of(dir: &Path, groups: &[&[&str]]) -> Vec<Item> {
    groups.iter().filter_map(|g| g.iter().find_map(|f| file_item(dir.join(f)))).collect()
}

/// Drops `//` and `/* */` comments, then trailing commas, outside strings, so JSONC parses.
pub(crate) fn strip_jsonc(text: &str) -> String {
    let mut bare = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        match (in_string, c, chars.peek()) {
            (true, '\\', _) => {
                bare.push(c);
                bare.extend(chars.next());
            }
            (_, '"', _) => {
                in_string = !in_string;
                bare.push(c);
            }
            (false, '/', Some('/')) => {
                chars.by_ref().find(|c| *c == '\n');
                bare.push('\n');
            }
            (false, '/', Some('*')) => {
                chars.next();
                let mut last = ' ';
                chars.by_ref().find(|c| std::mem::replace(&mut last, *c) == '*' && *c == '/');
            }
            _ => bare.push(c),
        }
    }
    let mut out = String::with_capacity(bare.len());
    let mut in_string = false;
    let mut chars = bare.chars().peekable();
    while let Some(c) = chars.next() {
        match (in_string, c) {
            (true, '\\') => {
                out.push(c);
                out.extend(chars.next());
            }
            (_, '"') => {
                in_string = !in_string;
                out.push(c);
            }
            (false, ',') => {
                let next = chars.clone().find(|c| !c.is_whitespace());
                if !matches!(next, Some('}' | ']')) {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Servers under `key` in a JSON(C) or TOML config; `projects.<cwd>` too for `~/.claude.json`.
fn mcp_servers(path: &Path, key: &str, cwd: Option<&Path>) -> Vec<(String, Option<Status>)> {
    let Ok(text) = fs::read_to_string(path) else { return vec![] };
    if path.extension().is_some_and(|e| e == "toml") {
        return toml_tables(&text, key).into_iter().map(|n| (n, None)).collect();
    }
    let Ok(json) = serde_json::from_str::<Value>(&strip_jsonc(&text)) else { return vec![] };
    let servers = |v: &Value| -> Vec<(String, Option<Status>)> {
        v.as_object()
            .into_iter()
            .flatten()
            .map(|(name, cfg)| {
                let off = cfg["enabled"] == false || cfg["disabled"] == true;
                (name.clone(), off.then_some(Status::Disabled))
            })
            .collect()
    };
    let mut found = servers(&json[key]);
    if let Some(cwd) = cwd {
        found.extend(servers(&json["projects"][cwd.to_string_lossy().as_ref()][key]));
    }
    found
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

/// An agent file (`.md` frontmatter or `.toml`), or a folder holding `agent.md`.
fn agent_item(path: PathBuf) -> Option<Item> {
    let path = if path.is_dir() { path.join("agent.md") } else { path };
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
    let stem = || {
        let named = if path.ends_with("agent.md") { path.parent()? } else { &path };
        Some(named.file_stem()?.to_string_lossy().into_owned())
    };
    let name = field("name").or_else(stem)?;
    let description = field("description").unwrap_or_default();
    let tokens = (name.len() + description.len()) as u64 / 4;
    Some(Item { name, path, tokens, status: None })
}

fn push_unique(items: &mut Vec<Item>, item: Item) {
    if !items.iter().any(|i| i.name == item.name) {
        items.push(item);
    }
}

/// Reads what `provider` loads for a session in `cwd` from its config files: memory files,
/// MCP servers, custom agents and skills. Token counts are estimates.
pub fn scan(provider: Provider, cwd: Option<&Path>, home: &Path, skills: &[Skill]) -> Inventory {
    let Some(l) = layout(provider, home) else {
        return Inventory {
            skills: skill_items(skills, &[SkillSource::Aether]),
            ..Inventory::default()
        };
    };
    let dirs = cwd.map(|c| project_dirs(c, l.to_root)).unwrap_or_default();
    let mut inv = Inventory::default();

    let home_memory = first_of(&l.base, l.memory_home);
    let project_memory = dirs.iter().flat_map(|d| first_of(d, l.memory_project));
    let auto_memory = (provider == Provider::Claude)
        .then(|| {
            let slug = claude_slug(cwd?);
            file_item(home.join(".claude/projects").join(slug).join("memory/MEMORY.md"))
        })
        .flatten();
    for mut item in home_memory.into_iter().chain(project_memory).chain(auto_memory) {
        item.name = item.path.strip_prefix(home).map_or_else(
            |_| item.path.to_string_lossy().into_owned(),
            |rel| format!("~/{}", rel.to_string_lossy()),
        );
        inv.memory.push(item);
    }

    let home_mcp = l.mcp_home.iter().map(|(f, k)| (l.base.join(f), *k));
    let project_mcp =
        dirs.iter().flat_map(|d| l.mcp_project.iter().map(move |(f, k)| (d.join(f), *k)));
    for (path, key) in home_mcp.chain(project_mcp) {
        for (name, status) in mcp_servers(&path, key, cwd) {
            push_unique(&mut inv.mcp, Item { name, path: path.clone(), tokens: 0, status });
        }
    }

    let home_agents = l.agents_home.iter().map(|d| l.base.join(d));
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
            status: None,
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/inventory.rs"]
mod tests;
