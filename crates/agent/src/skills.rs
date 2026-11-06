use std::fs;
use std::path::{Path, PathBuf};

use adk_rust::skill::{SkillDraft, parse_skill_markdown};
use aether_core::{Skill, SkillSource, valid_skill_name};

const FILE: &str = "SKILL.md";

/// Where each source keeps its `<name>/SKILL.md` folders.
pub fn dir(source: SkillSource) -> Option<PathBuf> {
    match source {
        SkillSource::Aether => aether_core::config_dir().map(|dir| dir.join("skills")),
        SkillSource::ClaudeCode => std::env::home_dir().map(|home| home.join(".claude/skills")),
        SkillSource::Antigravity => std::env::home_dir().map(|home| home.join(".gemini/skills")),
        SkillSource::OpenCode => {
            std::env::home_dir().map(|home| home.join(".config/opencode/skills"))
        }
        SkillSource::Codex => std::env::home_dir().map(|home| home.join(".codex/skills")),
        SkillSource::Agents => std::env::home_dir().map(|home| home.join(".agents/skills")),
    }
}

/// Every readable skill from every source, sorted by source then name.
/// Unparseable skills are skipped with a log line.
pub fn load_all() -> Vec<Skill> {
    let mut skills: Vec<Skill> = SkillSource::ALL
        .into_iter()
        .flat_map(|source| {
            let dirs: Vec<PathBuf> = match source {
                SkillSource::Antigravity => {
                    let mut d = vec![];
                    if let Some(home) = std::env::home_dir() {
                        d.push(home.join(".gemini/skills"));
                        d.push(home.join(".gemini/antigravity-cli/builtin/skills"));
                        d.push(home.join(".gemini/config/plugins/superpowers/skills"));
                    }
                    d
                }
                SkillSource::OpenCode => {
                    let mut d = vec![];
                    if let Some(home) = std::env::home_dir() {
                        d.push(home.join(".config/opencode/skills"));
                        d.push(home.join(".opencode/skills"));
                    }
                    d
                }
                SkillSource::ClaudeCode => {
                    let mut d = vec![];
                    if let Some(home) = std::env::home_dir() {
                        d.push(home.join(".claude/skills"));
                        d.push(home.join(".config/claude/skills"));
                    }
                    d
                }
                SkillSource::Codex => {
                    let mut d = vec![];
                    if let Some(home) = std::env::home_dir() {
                        d.push(home.join(".codex/skills"));
                        d.push(home.join(".config/codex/skills"));
                    }
                    d
                }
                _ => dir(source).into_iter().collect(),
            };
            dirs.into_iter().filter_map(|d| fs::read_dir(d).ok()).flat_map(move |entries| {
                entries.filter_map(Result::ok).filter_map(move |entry| load(&entry.path(), source))
            })
        })
        .collect();
    skills.sort_by(|a, b| (a.source as u8, &a.name).cmp(&(b.source as u8, &b.name)));
    skills.dedup_by(|a, b| a.name == b.name);
    skills
}

fn load(dir: &Path, source: SkillSource) -> Option<Skill> {
    let path = dir.join(FILE);
    let content = fs::read_to_string(&path).ok()?;
    let parsed = parse_skill_markdown(&path, &content)
        .or_else(|_| parse_skill_markdown(&path, &without_tool_list(&content)));
    match parsed {
        Ok(parsed) => Some(Skill {
            name: parsed.name,
            description: parsed.description,
            instructions: parsed.body,
            source,
            dir: dir.to_path_buf(),
        }),
        Err(e) => {
            eprintln!("skipping skill {}: {e}", path.display());
            None
        }
    }
}

/// Claude Code accepts `allowed-tools: Read, Bash` as one string; adk wants a
/// list. Aether doesn't use the field, so drop that form rather than the skill.
fn without_tool_list(content: &str) -> String {
    content
        .lines()
        .filter(|line| {
            !line.strip_prefix("allowed-tools:").is_some_and(|value| !value.trim().is_empty())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Writes an Aether skill, renaming its folder when `previous` had another
/// name. Other files in the folder are kept.
pub fn save(
    previous: Option<&Skill>,
    name: &str,
    description: &str,
    instructions: &str,
) -> Result<Skill, String> {
    let (name, description) = (name.trim(), description.trim());
    if !valid_skill_name(name) {
        return Err("Use lowercase letters, numbers and single hyphens.".into());
    }
    if description.is_empty() {
        return Err("Add a description so the assistant knows when to use it.".into());
    }
    let root = dir(SkillSource::Aether).ok_or("No config folder on this system.")?;
    let target = root.join(name);
    let renaming = previous.is_some_and(|p| p.dir != target);
    if (previous.is_none() || renaming) && target.exists() {
        return Err(format!("A skill named “{name}” already exists."));
    }
    let markdown = SkillDraft::new(name, description)
        .with_body(instructions.trim())
        .to_markdown()
        .map_err(|e| e.to_string())?;
    let write = || -> std::io::Result<()> {
        match previous {
            Some(previous) if renaming => fs::rename(&previous.dir, &target)?,
            _ => fs::create_dir_all(&target)?,
        }
        fs::write(target.join(FILE), markdown)
    };
    write().map_err(|e| e.to_string())?;
    load(&target, SkillSource::Aether).ok_or_else(|| "Saved, but couldn't read it back.".into())
}

pub fn delete(skill: &Skill) -> std::io::Result<()> {
    if !skill.source.editable() {
        return Err(std::io::Error::other("skills from other tools are read-only"));
    }
    fs::remove_dir_all(&skill.dir)
}

#[cfg(test)]
#[path = "../tests/unit/skills.rs"]
mod tests;
