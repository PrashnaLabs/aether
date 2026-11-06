//! Plain-file notes operations. Call filesystem and Git operations on a worker thread.
use chrono::Local;
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime},
};

type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    Named,
    Docs,
    Daily,
    Scratchpad,
    Credentials,
    Password,
    Timestamped,
}
#[derive(Clone, Debug, Default)]
pub struct NoteQuery {
    pub filename: String,
    pub content: String,
    pub tag: String,
    pub backlink: String,
    pub recent: Option<usize>,
    pub include_archive: bool,
}
#[derive(Clone, Debug)]
pub struct NoteEntry {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub modified: SystemTime,
}
#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
    scope: PathBuf,
    last_note_file: PathBuf,
}
impl Store {
    pub fn new(root: PathBuf) -> Self {
        let root = if root.is_absolute() {
            root
        } else {
            std::env::current_dir().unwrap_or_default().join(root)
        };
        // Resolve aliases above the notes tree (notably /tmp and /var on macOS),
        // even when the notes directory itself has not been created yet.
        let mut existing = root.as_path();
        let mut missing = Vec::new();
        while !existing.exists() {
            let Some(name) = existing.file_name() else { break };
            missing.push(name.to_owned());
            let Some(parent) = existing.parent() else { break };
            existing = parent;
        }
        let root = match existing.canonicalize() {
            Ok(mut resolved) => {
                for name in missing.iter().rev() {
                    resolved.push(name);
                }
                resolved
            }
            Err(_) => root,
        };
        Self {
            scope: root.clone(),
            root,
            last_note_file: std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".last_note"),
        }
    }
    #[cfg(test)]
    pub fn with_last_note_file(mut self, path: PathBuf) -> Self {
        self.last_note_file = path;
        self
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn scope_root(&self) -> &Path {
        &self.scope
    }
    pub fn scoped(&self, project: Option<&str>) -> Result<Self> {
        let mut store = self.clone();
        store.scope = match project {
            None => self.root.clone(),
            Some(name) => {
                if name.is_empty()
                    || name.starts_with(['.', '-'])
                    || name.contains("..")
                    || name.contains(['/', '\\'])
                {
                    return Err("Invalid project name".into());
                }
                self.root.join("projects").join(name)
            }
        };
        store.checked(&store.scope)?;
        Ok(store)
    }
    // Reject links even when their target happens to be inside the tree. This also
    // checks existing ancestors of a destination that has not been created yet.
    fn checked(&self, path: &Path) -> Result<PathBuf> {
        let path = if path.is_absolute() { path.to_owned() } else { self.scope.join(path) };
        if path.components().any(|c| matches!(c, Component::ParentDir))
            || !path.starts_with(&self.root)
        {
            return Err("Note path escapes the notes directory".into());
        }
        let mut ancestor = PathBuf::new();
        for part in path.components() {
            ancestor.push(part.as_os_str());
            match fs::symlink_metadata(&ancestor) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err("Symlink note paths are not allowed".into());
                }
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(path)
    }
    fn named(&self, name: &str) -> Result<PathBuf> {
        if name.trim().is_empty() {
            return Err("Enter a note name".into());
        }
        let mut name = PathBuf::from(name);
        if name.extension().is_none() {
            name.set_extension("md");
        }
        let path = if name.components().count() == 1 && self.scope == self.root {
            self.scope.join("misc").join(name)
        } else {
            self.scope.join(name)
        };
        self.checked(&path)
    }
    pub fn create(
        &self,
        kind: NoteKind,
        name: &str,
        template: Option<&str>,
        initial: &str,
    ) -> Result<PathBuf> {
        let date = Local::now();
        let path = match kind {
            NoteKind::Named => self.named(name)?,
            NoteKind::Docs => {
                let mut p = PathBuf::from(name);
                if name.trim().is_empty() {
                    return Err("Enter a document name".into());
                }
                if p.extension().is_none() {
                    p.set_extension("md");
                }
                self.checked(&self.scope.join("docs").join(p))?
            }
            NoteKind::Daily | NoteKind::Scratchpad => self.checked(
                &self
                    .scope
                    .join(if kind == NoteKind::Daily { "daily" } else { "scratchpad" })
                    .join(format!("{}.md", date.format("%Y-%m-%d"))),
            )?,
            NoteKind::Credentials | NoteKind::Password => {
                if name.is_empty() {
                    return Err("Enter a credential name".into());
                }
                self.checked(&self.scope.join("creds").join(format!(
                    "{name}.{}.md",
                    if kind == NoteKind::Credentials { "creds" } else { "password" }
                )))?
            }
            NoteKind::Timestamped => {
                let dir = if name.is_empty() || name == "." || name == "misc" {
                    if self.scope == self.root {
                        self.scope.join("misc")
                    } else {
                        self.scope.clone()
                    }
                } else {
                    self.scope.join(name)
                };
                self.unique(
                    &self.checked(&dir.join(format!("{}.md", date.format("%Y-%m-%d_%H-%M-%S"))))?,
                )?
            }
        };
        self.checked(&path)?;
        fs::create_dir_all(path.parent().ok_or("Invalid destination")?)
            .map_err(|e| e.to_string())?;
        let existing = match fs::metadata(&path) {
            Ok(m) => {
                if !m.is_file() {
                    return Err("Destination is not a file".into());
                }
                m.len()
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e.to_string()),
        };
        let mut text = String::new();
        if existing == 0 {
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("md");
            let template_name =
                template.or(if kind == NoteKind::Named { Some("template") } else { None });
            if let Some(name) = template_name {
                let tpl =
                    self.checked(&self.root.join(".templates").join(format!("{name}.{ext}")))?;
                match fs::read_to_string(tpl) {
                    Ok(t) => text.push_str(&t),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.to_string()),
                }
            }
            if kind != NoteKind::Scratchpad {
                text.push_str(initial);
            }
        }
        if kind == NoteKind::Scratchpad {
            text.push_str(&format!("\n### {}\n", date.format("%Y-%m-%d %H:%M:%S")));
            text.push_str(initial);
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        Ok(path)
    }
    /// Append captured text verbatim; unlike create, capture never discards text
    /// when the chosen note already contains content.
    pub fn capture(&self, name: &str, text: &str) -> Result<PathBuf> {
        let path = self.create(NoteKind::Named, name, None, "")?;
        self.checked(&path)?;
        let mut file =
            fs::OpenOptions::new().append(true).open(&path).map_err(|e| e.to_string())?;
        file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        Ok(path)
    }

    fn walk(&self, dir: &Path, output: &mut Vec<PathBuf>, archive: bool) -> Result<()> {
        self.checked(dir)?;
        if !dir.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let ty = entry.file_type().map_err(|e| e.to_string())?;
            if ty.is_symlink() {
                continue;
            }
            if ty.is_dir() {
                let name = entry.file_name();
                if name == ".trash"
                    || name == ".templates"
                    || name == ".git"
                    || (!archive && name == "archive")
                {
                    continue;
                }
                self.walk(&path, output, archive)?;
            } else if ty.is_file()
                && matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("md" | "txt" | "json" | "text")
                )
            {
                output.push(path);
            }
        }
        Ok(())
    }
    pub fn list(&self, query: &NoteQuery) -> Result<Vec<NoteEntry>> {
        let mut paths = vec![];
        self.walk(&self.scope, &mut paths, query.include_archive)?;
        let mut result = vec![];
        for path in paths {
            let relative_path =
                path.strip_prefix(&self.scope).map_err(|e| e.to_string())?.to_owned();
            if !relative_path
                .to_string_lossy()
                .to_lowercase()
                .contains(&query.filename.to_lowercase())
            {
                continue;
            }
            if !query.content.is_empty() || !query.tag.is_empty() || !query.backlink.is_empty() {
                let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                if !text.to_lowercase().contains(&query.content.to_lowercase()) {
                    continue;
                }
                if !query.tag.is_empty()
                    && !text.lines().filter_map(|l| l.strip_prefix("tags:")).any(|tags| {
                        tags.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                            .any(|t| t == query.tag)
                    })
                {
                    continue;
                }
                if !query.backlink.is_empty() {
                    let name = Path::new(&query.backlink)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&query.backlink);
                    if !text.contains(&format!("[[{name}]]")) {
                        continue;
                    }
                }
            }
            let modified =
                fs::metadata(&path).and_then(|m| m.modified()).map_err(|e| e.to_string())?;
            result.push(NoteEntry { path, relative_path, modified });
        }
        if let Some(limit) = query.recent {
            result.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
            result.truncate(limit);
        } else {
            result.sort_by(|a, b| a.path.cmp(&b.path));
        }
        Ok(result)
    }
    pub fn remember(&self, path: &Path) -> Result<()> {
        let path = self.checked(path)?;
        if !path.is_file() {
            return Err("Note does not exist".into());
        }
        fs::write(&self.last_note_file, format!("{}\n", path.display())).map_err(|e| e.to_string())
    }
    pub fn last(&self) -> Result<PathBuf> {
        let text = fs::read_to_string(&self.last_note_file).map_err(|e| e.to_string())?;
        let path = self.checked(Path::new(text.trim_end_matches(['\r', '\n'])))?;
        if !path.is_file() {
            return Err("Last note no longer exists".into());
        }
        Ok(path)
    }
    pub fn templates(&self) -> Result<Vec<PathBuf>> {
        let dir = self.checked(&self.root.join(".templates"))?;
        let mut files = vec![];
        self.walk(&dir, &mut files, true)?;
        files.sort();
        Ok(files)
    }
    fn unique(&self, path: &Path) -> Result<PathBuf> {
        let path = self.checked(path)?;
        if !path.exists() {
            return Ok(path);
        }
        for n in 1..10000 {
            let candidate = path.with_file_name(format!(
                "{}_{n}.{}",
                path.file_stem().unwrap_or_default().to_string_lossy(),
                path.extension().unwrap_or_default().to_string_lossy()
            ));
            self.checked(&candidate)?;
            if !candidate.exists() {
                return Ok(candidate);
            }
        }
        Err("Too many filename collisions".into())
    }
    fn move_file(&self, source: &Path, destination: &Path) -> Result<PathBuf> {
        let source = self.checked(source)?;
        let destination = self.checked(destination)?;
        if !source.is_file() {
            return Err("Note does not exist".into());
        }
        if destination.exists() {
            return Err("Destination already exists".into());
        }
        fs::create_dir_all(destination.parent().ok_or("Invalid destination")?)
            .map_err(|e| e.to_string())?;
        // hard_link creates the destination atomically without overwriting a conflict.
        let was_last = self.last().ok().as_ref() == Some(&source);
        fs::hard_link(&source, &destination).map_err(|e| e.to_string())?;
        fs::remove_file(&source).map_err(|e| e.to_string())?;
        if was_last {
            self.remember(&destination)?;
        }
        Ok(destination)
    }
    pub fn rename(&self, source: &Path, name: &str) -> Result<PathBuf> {
        self.move_file(source, &self.named(name)?)
    }
    pub fn archive(&self, source: &Path) -> Result<PathBuf> {
        let source = self.checked(source)?;
        let rel = source.strip_prefix(&self.root).map_err(|e| e.to_string())?;
        if rel.starts_with("archive") || rel.starts_with(".trash") || rel.starts_with(".templates")
        {
            return Err("Cannot archive this path".into());
        }
        self.move_file(&source, &self.root.join("archive").join(rel))
    }
    pub fn trash(&self, source: &Path) -> Result<PathBuf> {
        let source = self.checked(source)?;
        let relative = source.strip_prefix(&self.root).map_err(|e| e.to_string())?;
        if relative.starts_with(".trash") || relative.starts_with(".templates") {
            return Err("Cannot trash this path".into());
        }
        let dest = self.unique(&self.root.join(".trash").join(format!(
            "{}_{}",
            Local::now().format("%Y%m%d_%H%M%S"),
            source.file_name().ok_or("Invalid filename")?.to_string_lossy()
        )))?;
        let metadata = self.checked(&dest.with_extension(format!(
            "{}.origin.json",
            dest.extension().unwrap_or_default().to_string_lossy()
        )))?;
        let encoded =
            serde_json::to_vec(&relative.to_string_lossy().as_ref()).map_err(|e| e.to_string())?;
        let mut record = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open({
                fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
                &metadata
            })
            .map_err(|e| e.to_string())?;
        record.write_all(&encoded).map_err(|e| e.to_string())?;
        match self.move_file(&source, &dest) {
            Ok(p) => Ok(p),
            Err(e) => {
                let _ = fs::remove_file(metadata);
                Err(e)
            }
        }
    }
    pub fn trash_list(&self) -> Result<Vec<PathBuf>> {
        let dir = self.checked(&self.root.join(".trash"))?;
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut out = vec![];
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_file()
                && !entry.file_name().to_string_lossy().ends_with(".origin.json")
            {
                out.push(entry.path());
            }
        }
        out.sort();
        Ok(out)
    }
    /// Permanently remove flat trash entries after explicit user confirmation.
    /// Do not traverse directories or follow links placed inside the trash.
    pub fn empty_trash(&self) -> Result<usize> {
        let dir = self.checked(&self.root.join(".trash"))?;
        if !dir.exists() {
            return Ok(0);
        }
        let mut files = Vec::new();
        for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_file() {
                files.push(self.checked(&entry.path())?);
            }
        }
        let mut count = 0;
        for path in files {
            self.checked(&path)?;
            fs::remove_file(&path).map_err(|e| e.to_string())?;
            if !path.file_name().unwrap_or_default().to_string_lossy().ends_with(".origin.json") {
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn restore(&self, source: &Path) -> Result<PathBuf> {
        let source = self.checked(source)?;
        if source.parent() != Some(self.root.join(".trash").as_path()) {
            return Err("Select a file in trash".into());
        }
        let metadata = self.checked(&source.with_extension(format!(
            "{}.origin.json",
            source.extension().unwrap_or_default().to_string_lossy()
        )))?;
        let destination = if metadata.exists() {
            let relative: String =
                serde_json::from_slice(&fs::read(&metadata).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            self.checked(&self.root.join(relative))?
        } else {
            let name = source.file_name().ok_or("Invalid trash file")?.to_string_lossy();
            let base = if name.len() > 16
                && name.as_bytes()[8] == b'_'
                && name.as_bytes()[15] == b'_'
                && name.as_bytes()[..8].iter().all(|c| c.is_ascii_digit())
                && name.as_bytes()[9..15].iter().all(|c| c.is_ascii_digit())
            {
                &name[16..]
            } else {
                &name
            };
            self.named(base)?
        };
        let result = self.move_file(&source, &destination)?;
        if metadata.exists() {
            fs::remove_file(metadata).map_err(|e| e.to_string())?;
        }
        Ok(result)
    }
    pub fn archive_older_than(&self, days: u64) -> Result<usize> {
        let cutoff = SystemTime::now()
            .checked_sub(Duration::from_secs(days.saturating_mul(86400)))
            .ok_or("Invalid age")?;
        let mut count = 0;
        for note in self.list(&NoteQuery::default())? {
            if note.modified < cutoff {
                self.archive(&note.path)?;
                count += 1;
            }
        }
        Ok(count)
    }
    pub fn clean_empty(&self) -> Result<usize> {
        let mut count = 0;
        for note in self.list(&NoteQuery::default())? {
            if fs::metadata(&note.path).map_err(|e| e.to_string())?.len() == 0 {
                self.trash(&note.path)?;
                count += 1;
            }
        }
        Ok(count)
    }
    pub fn sync(&self) -> Result<String> {
        self.checked(&self.root)?;
        if !self.root.join(".git").exists() {
            return Err("Notes directory is not a Git repository; run git init there first".into());
        }
        let run = |args: &[&str]| -> Result<String> {
            let out = Command::new("git")
                .current_dir(&self.root)
                .args(args)
                .output()
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(format!(
                    "git {}: {}",
                    args.join(" "),
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
        };
        run(&["add", "-A"])?;
        if !run(&["diff", "--cached", "--name-only"])?.is_empty() {
            run(&["commit", "-m", &format!("sync: {}", Local::now().format("%Y-%m-%d %H:%M:%S"))])?;
        }
        let remotes = run(&["remote"])?;
        if remotes.lines().any(|r| r == "origin") {
            let branch = run(&["branch", "--show-current"])?;
            if branch.is_empty() {
                return Err("Cannot sync a detached Git HEAD".into());
            }
            run(&["pull", "--rebase", "origin", &branch])?;
            run(&["push", "origin", &branch])?;
            Ok("Notes synced".into())
        } else {
            Ok("Notes committed locally; no origin remote configured".into())
        }
    }
}
#[cfg(test)]
#[path = "../../tests/unit/notes/store.rs"]
mod tests;
