use std::path::{Path, PathBuf};

use aether_core::{OrgHire, Provider, Settings};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Org {
    pub project: String,
    pub heads: Vec<Head>,
    pub requests: Vec<Request>,
    pub goal: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Head {
    pub role: String,
    pub title: String,
    pub owns: String,
    pub provider: Provider,
    pub model: String,
    pub effort: String,
    /// Role that reviews this head's work; `None` is the user.
    pub reviewer: Option<String>,
    pub max_workers: usize,
    pub chat: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Request {
    pub id: u64,
    pub head: String,
    pub title: String,
    pub why: String,
    pub card: Option<i64>,
    pub provider: Provider,
    pub model: String,
    pub task: String,
    pub checks: Vec<(String, bool)>,
    pub created_ms: i64,
    /// Queued by the user; starts when a slot frees.
    pub queued: bool,
}

pub(crate) struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub roles: &'static [&'static str],
}

/// What the wizard collected; `org_create` builds the org from it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OrgSpec {
    pub template: String,
    pub heads: Vec<Head>,
    pub goal: String,
}

/// A worker to hire, from the hire dialog or a head's `request_worker`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HireSpec {
    pub head: String,
    pub card: Option<i64>,
    pub title: String,
    pub task: String,
    pub why: String,
    pub provider: Provider,
    pub model: String,
    pub effort: String,
    pub worktree: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Approve,
    Queue,
    Deny,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NeedsYou {
    Hire(u64),
    Waiting { chat: i64 },
    Review { card: i64 },
}

/// A `message_agent` note between org chats.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OrgMessage {
    pub from: i64,
    pub to: i64,
    pub text: String,
    pub at_ms: i64,
}

pub(crate) const TEMPLATES: [Template; 4] = [
    Template {
        id: "solo",
        name: "Solo builder",
        desc: "A planner and one engineering head. Cheapest; good for small repos.",
        roles: &["CEO", "ENG"],
    },
    Template {
        id: "startup",
        name: "Startup",
        desc: "Engineering, product and security heads. What the 2026-11-12 trial ran.",
        roles: &["CEO", "ENG", "PRODUCT", "SEC"],
    },
    Template {
        id: "company",
        name: "Full company",
        desc: "Adds design, QA, release ops and marketing. Expensive; merge roles you do not need.",
        roles: &["CEO", "ENG", "PRODUCT", "SEC", "DESIGN", "QA", "OPS", "MKT"],
    },
    Template {
        id: "custom",
        name: "Custom",
        desc: "Start from just a CEO and add heads yourself.",
        roles: &["CEO"],
    },
];

/// role, title, owns, provider, model, reviewer (`None` = the user)
type RoleDef =
    (&'static str, &'static str, &'static str, Provider, &'static str, Option<&'static str>);

pub(crate) const ROLES: [RoleDef; 8] = [
    (
        "CEO",
        "CEO",
        "Intake, routing, roster, hiring gate, status to you",
        Provider::Claude,
        "sonnet",
        None,
    ),
    (
        "ENG",
        "Engineering",
        "Design, implementation through workers, code review",
        Provider::Codex,
        "gpt-6.1-sol",
        Some("SEC"),
    ),
    (
        "PRODUCT",
        "Product",
        "Requirements, acceptance criteria, docs, release notes",
        Provider::Antigravity,
        "gemini-3.8-flash-high",
        Some("ENG"),
    ),
    ("SEC", "Security", "Threat review, release gate", Provider::Claude, "opus", Some("ENG")),
    (
        "DESIGN",
        "Design",
        "UI specs, prototypes, UX review",
        Provider::Claude,
        "sonnet",
        Some("PRODUCT"),
    ),
    (
        "QA",
        "QA",
        "Test plans, regression runs, bug triage",
        Provider::Codex,
        "gpt-6-luna",
        Some("ENG"),
    ),
    (
        "OPS",
        "Release ops",
        "Builds, packaging, CI, release checklist",
        Provider::Codex,
        "gpt-6.1-terra",
        Some("SEC"),
    ),
    (
        "MKT",
        "Marketing",
        "Website, launch copy, changelog voice",
        Provider::Antigravity,
        "gemini-3.8-flash-high",
        Some("PRODUCT"),
    ),
];

/// Model choices per provider, cheapest tier last.
pub(crate) fn models(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &["sonnet", "opus", "haiku", "fable"],
        Provider::Codex => &["gpt-6.1-sol", "gpt-6.1-terra", "gpt-6-luna"],
        Provider::Antigravity => {
            &["gemini-3.8-flash-high", "gemini-4-argon-high", "claude-sonnet-5-5-high"]
        }
        Provider::OpenCode => &["google/gemini-3.8-flash", "opencode/claude-sonnet-5-5"],
        Provider::Pi => &["opencode/claude-haiku-5-5", "opencode/gemini-3.8-flash"],
        Provider::OpenRouter => &[],
    }
}

/// Avatar colour as 0xRRGGBB; custom roles are grey.
pub(crate) fn role_color(role: &str) -> u32 {
    match role {
        "CEO" => 0xf5a524,
        "ENG" => 0x58a6ff,
        "PRODUCT" => 0xb18cff,
        "SEC" => 0xf85149,
        "DESIGN" => 0xff7ab6,
        "QA" => 0x3fb950,
        "OPS" => 0x2dd4bf,
        "MKT" => 0xfb923c,
        _ => 0x888888,
    }
}

/// Avatar text: up to three letters, PRODUCT shows as PM.
pub(crate) fn role_badge(role: &str) -> String {
    if role == "PRODUCT" { "PM".into() } else { role.chars().take(3).collect() }
}

pub(crate) fn template(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}

impl Head {
    /// Built-in defaults for `role`; an unknown role is a custom head titled after it.
    pub(crate) fn new(role: &str, max_workers: usize) -> Self {
        let def = ROLES.iter().find(|r| r.0 == role);
        let (title, owns, provider, model, reviewer) = match def {
            Some(&(_, title, owns, provider, model, reviewer)) => {
                (title.to_string(), owns.to_string(), provider, model, reviewer)
            }
            None => (role.to_string(), String::new(), Provider::Claude, "sonnet", None),
        };
        Self {
            role: role.into(),
            title,
            owns,
            provider,
            model: model.into(),
            effort: String::new(),
            reviewer: reviewer.map(Into::into),
            max_workers,
            chat: None,
        }
    }

    /// Head chat title, `[ENG] Engineering`.
    pub(crate) fn chat_title(&self) -> String {
        format!("[{}] {}", self.role, self.title)
    }
}

impl OrgSpec {
    pub(crate) fn from_template(id: &str, max_workers: usize) -> Self {
        let roles = template(id).map_or(&["CEO"][..], |t| t.roles);
        Self {
            template: id.into(),
            heads: roles.iter().map(|r| Head::new(r, max_workers)).collect(),
            goal: String::new(),
        }
    }
}

impl Org {
    pub(crate) fn head(&self, role: &str) -> Option<&Head> {
        self.heads.iter().find(|h| h.role == role)
    }

    pub(crate) fn head_mut(&mut self, role: &str) -> Option<&mut Head> {
        self.heads.iter_mut().find(|h| h.role == role)
    }

    pub(crate) fn head_of_chat(&self, chat: i64) -> Option<&Head> {
        self.heads.iter().find(|h| h.chat == Some(chat))
    }

    /// Head chat named by a `<prefix>:<role>` card label.
    pub(crate) fn labelled(&self, labels: &[String], prefix: &str) -> Option<i64> {
        labels.iter().find_map(|l| {
            let role = l.strip_prefix(prefix)?.strip_prefix(':')?;
            self.heads.iter().find(|h| h.role.eq_ignore_ascii_case(role))?.chat
        })
    }

    /// Chats to wake when `author` notes a card: its owning head and the CEO.
    pub(crate) fn note_wake(&self, labels: &[String], author: i64) -> Vec<i64> {
        let mut out = Vec::new();
        for chat in [self.labelled(labels, "dept"), self.ceo_chat()].into_iter().flatten() {
            if chat != author && !out.contains(&chat) {
                out.push(chat);
            }
        }
        out
    }

    pub(crate) fn ceo_chat(&self) -> Option<i64> {
        self.head("CEO").and_then(|h| h.chat)
    }

    pub(crate) fn next_request_id(&self) -> u64 {
        self.requests.iter().map(|r| r.id).max().unwrap_or(0) + 1
    }
}

/// Folder name under `<config>/org/` for a project path.
pub(crate) fn slug(project: &str) -> String {
    let mut out = String::new();
    for c in project.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() { "project".into() } else { out.into() }
}

pub(crate) fn role_path(dir: &Path, role: &str) -> PathBuf {
    dir.join("roles").join(format!("{}.md", role.to_lowercase()))
}

pub(crate) fn memory_path(dir: &Path, role: &str) -> PathBuf {
    dir.join("memory").join(format!("{}.md", role.to_lowercase()))
}

/// Default persona markdown for a head's role file.
pub(crate) fn role_file_text(head: &Head) -> String {
    format!(
        "# Role: {}\n\n## Owns\n- {}\n\n## Rules\n- Re-read this file and memory/{}.md before each task.\n- Work is tracked on cards; put durable info in card notes.\n- Code is not reviewed until tests and lint pass.\n- Done is human-only.\n",
        head.title,
        head.owns,
        head.role.to_lowercase()
    )
}

/// First message of a head's session: who it is, who else is in the org, where its files are.
pub(crate) fn onboarding_prompt(org: &Org, head: &Head, role_text: &str, dir: &Path) -> String {
    let roster: Vec<String> = org
        .heads
        .iter()
        .filter(|h| h.role != head.role)
        .map(|h| {
            let chat = h.chat.map_or("not created yet".into(), |id| format!("chat {id}"));
            format!("- {} ({}): {chat}", h.title, h.role)
        })
        .collect();
    let reviewer = head.reviewer.as_deref().unwrap_or("the user");
    let boss = if head.role == "CEO" {
        "You are the CEO: turn the user's goals into cards, route them to the heads with message_agent, and report status to the user. Nothing runs until the user gives a goal.".to_string()
    } else {
        match org.ceo_chat() {
            Some(id) => format!("You report to the CEO, chat {id}. Message it with message_agent."),
            None => "You report to the user.".into(),
        }
    };
    format!(
        "You are the {title} head ({role}) of this project's organization in Aether.\n\n{boss}\n\nYour work is reviewed by {reviewer}. Hire workers with request_worker; never more than {max} at once. Done is human-only.\n\nOther heads:\n{roster}\n\nYour role file is {role_file}; your memory file is {memory_file}. Keep durable notes in memory, re-read both before each task.\n\n{role_text}\n\nReply with one line confirming your role, then wait for work.",
        title = head.title,
        role = head.role,
        max = head.max_workers,
        roster = if roster.is_empty() { "- none".into() } else { roster.join("\n") },
        role_file = role_path(dir, &head.role).display(),
        memory_file = memory_path(dir, &head.role).display(),
        role_text = role_text.trim(),
    )
}

/// The hiring rules, each with whether it passes.
pub(crate) fn hire_checks(
    head: &Head,
    card: Option<i64>,
    running: usize,
    pool_busy: usize,
    pool_cap: usize,
) -> Vec<(String, bool)> {
    vec![
        match card {
            Some(id) => (format!("Has card #{id}"), true),
            None => ("No card yet".into(), false),
        },
        (
            format!("{} at {running} of {} workers", head.role, head.max_workers),
            running < head.max_workers,
        ),
        (format!("Org pool {pool_busy}/{pool_cap} busy"), pool_busy < pool_cap),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HireOutcome {
    Hire,
    Deny,
    Pending,
}

/// What `request_worker` does under `policy` given the hiring checks.
pub(crate) fn hire_outcome(policy: OrgHire, checks: &[(String, bool)]) -> HireOutcome {
    let pass = checks.iter().all(|c| c.1);
    match policy {
        OrgHire::Ask => HireOutcome::Pending,
        _ if pass => HireOutcome::Hire,
        OrgHire::Rules => HireOutcome::Deny,
        OrgHire::Auto => HireOutcome::Pending,
    }
}

/// Org-wide worker cap: the setting, else half the cores or one per 5 GB, whichever is less.
pub(crate) fn pool_cap(settings: &Settings, cores: usize, mem_gb: usize) -> usize {
    settings.org_pool.unwrap_or((cores / 2).min(mem_gb / 5)).max(1)
}

/// Sender chat id and body of a `[from <title> #<id>] text` message.
pub(crate) fn parse_from(text: &str) -> Option<(i64, &str)> {
    let rest = text.strip_prefix("[from ")?;
    // head titles hold their own `[ROLE]`, so the tag ends at the first `#<id>]`
    rest.match_indices(']').find_map(|(at, _)| {
        let id = rest[..at].rsplit_once('#')?.1.parse().ok()?;
        Some((id, rest[at + 1..].trim_start()))
    })
}

#[cfg(test)]
#[path = "../../tests/unit/org/model.rs"]
mod tests;
