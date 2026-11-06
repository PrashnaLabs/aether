use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkillSource {
    Aether,
    ClaudeCode,
    Antigravity,
    OpenCode,
    Codex,
    Agents,
}

impl SkillSource {
    pub const ALL: [Self; 6] = [
        Self::Aether,
        Self::ClaudeCode,
        Self::Antigravity,
        Self::OpenCode,
        Self::Codex,
        Self::Agents,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Aether => "Aether",
            Self::ClaudeCode => "Claude Code",
            Self::Antigravity => "Antigravity",
            Self::OpenCode => "OpenCode",
            Self::Codex => "Codex",
            Self::Agents => "Agents",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Aether => "aether",
            Self::ClaudeCode => "claude",
            Self::Antigravity => "antigravity",
            Self::OpenCode => "opencode",
            Self::Codex => "codex",
            Self::Agents => "agents",
        }
    }

    /// Only Aether's own skills are written by the app; the others belong to
    /// other tools.
    pub fn editable(self) -> bool {
        self == Self::Aether
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub source: SkillSource,
    pub dir: PathBuf,
}

impl Skill {
    /// Stable across launches and unique across sources, for saving choices.
    pub fn id(&self) -> String {
        format!("{}:{}", self.source.key(), self.name)
    }
}

/// Lowercase letters, digits and single hyphens, as agentskills.io requires.
pub fn valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Skills the text invokes with `@name`, in order, without repeats.
pub fn mentioned_skills<'a>(text: &str, skills: &'a [Skill]) -> Vec<&'a Skill> {
    let mut found: Vec<&Skill> = Vec::new();
    for word in text.split_whitespace() {
        let Some(name) = word.strip_prefix('@') else {
            continue;
        };
        let name = name.trim_end_matches(|c: char| !c.is_ascii_alphanumeric());
        if let Some(skill) = skills.iter().find(|s| s.name == name)
            && !found.iter().any(|f| f.name == skill.name)
        {
            found.push(skill);
        }
    }
    found
}

/// The `@partial` being typed right before `cursor` (a byte offset), as the
/// byte offset of its `@` and the partial name.
pub fn mention_at(text: &str, cursor: usize) -> Option<(usize, &str)> {
    let before = text.get(..cursor)?;
    let at = before.rfind('@')?;
    let partial = &before[at + 1..];
    let starts_word = before[..at].chars().last().is_none_or(char::is_whitespace);
    let name_chars =
        partial.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    (starts_word && name_chars).then_some((at, partial))
}

/// Skills whose name contains `partial`, names starting with it first.
pub fn complete_skill<'a>(partial: &str, skills: &'a [Skill], limit: usize) -> Vec<&'a Skill> {
    let mut found: Vec<&Skill> = skills.iter().filter(|s| s.name.contains(partial)).collect();
    found.sort_by_key(|s| (!s.name.starts_with(partial), s.name.len(), &s.name));
    found.dedup_by(|a, b| a.name == b.name);
    found.truncate(limit);
    found
}

#[cfg(test)]
#[path = "../tests/unit/skill.rs"]
mod tests;
