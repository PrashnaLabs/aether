use std::path::Path;

use aether_core::complete_skill;
use gpui_kit::SharedString;

use super::{MAX_SUGGESTIONS, Pick};
use crate::app::ChatApp;

impl ChatApp {
    /// Mentions matching `query`: skills, subagent roles, and workspace files.
    pub(super) fn mentions(&self, query: &str) -> Vec<(SharedString, SharedString, Pick)> {
        let chat = &self.chats[self.active];
        let mut all: Vec<(String, String, String)> = Vec::new();
        let query_clean = query.trim().to_lowercase();

        // 1. Installed Skills
        let skills = self.agent_skills(chat.provider);
        for s in complete_skill(query, &skills, MAX_SUGGESTIONS * 2) {
            all.push((format!("@{}", s.name), s.description.clone(), format!("@{} ", s.name)));
        }

        // 2. Subagents & Agent roles (only for agent chats)
        if chat.is_agent() {
            let subagents = [
                ("research", "Research subagent: codebase, docs, web"),
                ("reviewer", "Code review & quality verification specialist"),
                ("self", "Subagent inheriting parent configuration"),
                ("image-generator", "Generates or edits images from brief"),
            ];
            for (name, desc) in subagents {
                if query_clean.is_empty() || name.contains(&query_clean) {
                    all.push((format!("@{name}"), desc.to_string(), format!("@{name} ")));
                }
            }
        }

        // 3. Workspace files
        if let Some(folder) = self.folder_of(chat) {
            for file in scan_project_files(&folder, query, 6) {
                all.push((format!("@{file}"), "Workspace file".to_string(), format!("@{file} ")));
            }
        }

        let mut found: Vec<(bool, (String, String, String))> = all
            .into_iter()
            .filter(|(label, desc, _)| {
                query_clean.is_empty()
                    || label.to_lowercase().contains(&query_clean)
                    || desc.to_lowercase().contains(&query_clean)
            })
            .map(|entry| (!entry.0.to_lowercase().starts_with(&format!("@{query_clean}")), entry))
            .collect();
        found.sort_by_key(|(later, _)| *later);
        found
            .into_iter()
            .take(MAX_SUGGESTIONS)
            .map(|(_, (label, description, replacement))| {
                (label.into(), description.into(), Pick::Insert(replacement))
            })
            .collect()
    }

    /// Terminal commands dynamically detected from the project workspace.
    pub(super) fn terminal_commands(&self, query: &str) -> Vec<(SharedString, SharedString, Pick)> {
        let chat = &self.chats[self.active];
        let folder = self.folder_of(chat);
        let cmds = detect_terminal_commands(folder.as_deref(), query);
        cmds.into_iter()
            .take(MAX_SUGGESTIONS)
            .map(|(cmd, desc)| {
                let label = format!("$ {cmd}");
                let replacement = format!("$ {cmd} ");
                (label.into(), desc.into(), Pick::Insert(replacement))
            })
            .collect()
    }

    /// Workspace files matching `#query`.
    pub(super) fn workspace_file_suggestions(
        &self,
        query: &str,
    ) -> Vec<(SharedString, SharedString, Pick)> {
        let chat = &self.chats[self.active];
        let Some(folder) = self.folder_of(chat) else {
            return Vec::new();
        };
        let files = scan_project_files(&folder, query, MAX_SUGGESTIONS);
        files
            .into_iter()
            .map(|file| {
                let label = format!("#{file}");
                let replacement = format!("#{file} ");
                (label.into(), "Workspace file".into(), Pick::Insert(replacement))
            })
            .collect()
    }
}

/// Scans workspace files recursively, avoiding heavy/ignored build directories.
fn scan_project_files(dir: &Path, query: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    scan_files_rec(dir, Path::new(""), query, &mut out, limit, 0);
    out
}

fn scan_files_rec(
    dir: &Path,
    rel: &Path,
    query: &str,
    out: &mut Vec<String>,
    limit: usize,
    depth: usize,
) {
    if depth > 4 || out.len() >= limit {
        return;
    }
    let full = if rel.as_os_str().is_empty() { dir.to_path_buf() } else { dir.join(rel) };
    let Ok(entries) = std::fs::read_dir(full) else {
        return;
    };
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.')
            || name_str == "target"
            || name_str == "node_modules"
            || name_str == "vendor"
            || name_str == "dist"
            || name_str == "build"
        {
            continue;
        }
        let child_rel = if rel.as_os_str().is_empty() {
            std::path::PathBuf::from(name)
        } else {
            rel.join(name)
        };
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            subdirs.push(child_rel);
        } else {
            let path_str = child_rel.display().to_string();
            if query.is_empty() || path_str.to_lowercase().contains(&query.to_lowercase()) {
                out.push(path_str);
                if out.len() >= limit {
                    return;
                }
            }
        }
    }
    for sub in subdirs {
        scan_files_rec(dir, &sub, query, out, limit, depth + 1);
        if out.len() >= limit {
            return;
        }
    }
}

/// Detects terminal commands dynamically from the active project workspace.
fn detect_terminal_commands(dir: Option<&Path>, query: &str) -> Vec<(String, String)> {
    let mut cmds = Vec::new();
    if let Some(d) = dir {
        if d.join("Cargo.toml").exists() {
            cmds.push(("cargo test", "Run workspace tests"));
            cmds.push(("cargo check", "Fast compilation and type check"));
            cmds.push(("cargo build", "Compile development artifacts"));
            cmds.push(("cargo clippy", "Run Clippy linter warnings"));
            cmds.push(("cargo run", "Run default package binary"));
        }
        if d.join("package.json").exists() {
            cmds.push(("npm test", "Run test suite"));
            cmds.push(("npm run dev", "Start local development server"));
            cmds.push(("npm run build", "Run build script"));
        }
        if d.join("pnpm-lock.yaml").exists() {
            cmds.push(("pnpm test", "Run pnpm test script"));
        }
        if d.join("pyproject.toml").exists() || d.join("requirements.txt").exists() {
            cmds.push(("pytest", "Run pytest test suite"));
            cmds.push(("python -m unittest", "Run Python unittest suite"));
        }
        if d.join(".git").exists() {
            cmds.push(("git status", "Show repository working tree status"));
            cmds.push(("git diff", "Inspect unstaged and staged changes"));
            cmds.push(("git log -n 5", "Show last 5 commits"));
            cmds.push(("git branch -a", "List local and remote branches"));
            cmds.push(("git pull", "Fetch and merge remote changes"));
        }
    }
    cmds.push(("ls -la", "List directory contents with details"));
    cmds.push(("pwd", "Print active working directory"));
    cmds.push(("make", "Run default Makefile target"));

    let q = query.trim().to_lowercase();
    cmds.into_iter()
        .filter(|(cmd, desc)| {
            q.is_empty() || cmd.to_lowercase().contains(&q) || desc.to_lowercase().contains(&q)
        })
        .map(|(c, d)| (c.to_string(), d.to_string()))
        .collect()
}

#[cfg(test)]
#[path = "../../tests/unit/suggest/sources.rs"]
mod tests;
