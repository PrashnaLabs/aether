use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use aether_core::{Provider, Role, home_dir};
use aether_driver::{claude, opencode};
use reqwest::Method;
use serde_json::Value;

use ExternalState::{Idle, Waiting, Working};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalState {
    Working,
    Waiting(String),
    Idle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExternalAgent {
    pub provider: Provider,
    /// None: an OpenCode session with no TUI process.
    pub pid: Option<u32>,
    /// `ps -o lstart=` text, for pid-reuse checks; "" when `pid` is None.
    pub started: String,
    pub elapsed: String,
    pub cwd: PathBuf,
    /// Claude sessionId, Pi session file path, or OpenCode `ses_` id; "" if unknown.
    pub session: String,
    pub title: String,
    pub state: ExternalState,
    /// State from a registry/API (true) or a transcript heuristic (false).
    pub exact: bool,
    pub origin: String,
}

#[derive(Clone, Debug, PartialEq)]
struct PsRow {
    pid: u32,
    ppid: u32,
    etime: String,
    started: String,
    command: String,
}

/// (id, directory, title, state, `time.updated` ms)
type OcSession = (String, PathBuf, String, ExternalState, Option<u64>);

/// Every Claude / Pi / OpenCode agent on this machine that aether did not
/// start. Blocking: runs `ps` and `lsof` and may query the OpenCode service.
/// `None`: `ps` failed, so the result says nothing about what is running.
pub fn scan(own_pid: u32, own_sessions: &HashSet<String>) -> Option<Vec<ExternalAgent>> {
    let ps = parse_ps(&run("ps", &["-axww", "-o", "pid=,ppid=,etime=,lstart=,command="])?);
    if ps.is_empty() {
        return None;
    }
    let rows = agent_rows(&ps, own_pid);
    let pids: Vec<String> = rows.iter().map(|(_, r, _)| r.pid.to_string()).collect();
    let cwds = (!pids.is_empty())
        .then(|| run("lsof", &["-a", "-p", &pids.join(","), "-d", "cwd", "-Fpn"]))
        .flatten()
        .map(|out| parse_lsof_cwd(&out))
        .unwrap_or_default();
    let agents = rows
        .into_iter()
        .map(|(provider, row, origin)| {
            let mut a = ExternalAgent {
                provider,
                pid: Some(row.pid),
                started: row.started,
                elapsed: row.etime,
                cwd: cwds.get(&row.pid).cloned().unwrap_or_default(),
                session: String::new(),
                title: String::new(),
                state: Idle,
                exact: false,
                origin,
            };
            match provider {
                Provider::Claude => claude_registry(&mut a, row.pid),
                Provider::Pi => pi_session(
                    &mut a,
                    &home_dir().join(".pi/agent/sessions"),
                    own_sessions,
                    &row.command,
                ),
                Provider::Codex => codex_session(&mut a, &home_dir().join(".codex"), own_sessions),
                Provider::Antigravity => antigravity_session(
                    &mut a,
                    &home_dir().join(".gemini/antigravity-cli"),
                    own_sessions,
                ),
                _ => {}
            }
            a
        })
        .collect();
    let now_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    Some(merge(agents, opencode_sessions(), own_sessions, now_ms))
}

// lsof escapes non-ASCII paths under the C locale a GUI app may inherit
fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .env("LC_ALL", "en_US.UTF-8")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn claude_registry(a: &mut ExternalAgent, pid: u32) {
    // only `<pid>.json`: the sibling `<pid>.*.key` files are credentials
    let path = claude::config_dir().join(format!("sessions/{pid}.json"));
    let Some((session, cwd, title, state, tmux)) =
        std::fs::read_to_string(path).ok().and_then(|j| claude_entry(&j, pid))
    else {
        return;
    };
    if !cwd.as_os_str().is_empty() {
        a.cwd = cwd;
    }
    if let Some(target) = tmux {
        a.origin = format!("tmux {target}");
    }
    (a.session, a.title, a.state, a.exact) = (session, title, state, true);
}

fn pi_session(a: &mut ExternalAgent, root: &Path, own: &HashSet<String>, command: &str) {
    if let Some(path) =
        pi_exact_path(a, root, command).filter(|p| !own.contains(p.to_string_lossy().as_ref()))
    {
        let quiet = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok())
            .map_or(0, |d| d.as_secs());
        a.state = pi_state(&tail(&path, 64 * 1024), quiet);
        a.session = path.to_string_lossy().into_owned();
        a.exact = true;
        return;
    }
    let cwd = a.cwd.to_string_lossy();
    let dir = pi_bucket(&cwd);
    let age = Duration::from_secs(etime_secs(&a.elapsed) + 1);
    let start = SystemTime::now().checked_sub(age).unwrap_or(SystemTime::UNIX_EPOCH);
    let newest = std::fs::read_dir(root.join(dir))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter(|e| !own.contains(e.path().to_string_lossy().as_ref()))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(mtime, _)| *mtime >= start)
        .max_by_key(|(mtime, _)| *mtime);
    let Some((mtime, path)) = newest else { return };
    let quiet = mtime.elapsed().map_or(0, |d| d.as_secs());
    a.state = pi_state(&tail(&path, 64 * 1024), quiet);
    a.session = path.to_string_lossy().into_owned();
}

fn pi_bucket(cwd: &str) -> String {
    format!("--{}--", cwd.strip_prefix('/').unwrap_or(cwd).replace(['/', ':'], "-"))
}

fn pi_exact_path(a: &ExternalAgent, root: &Path, command: &str) -> Option<PathBuf> {
    if let Some(path) = pi_session_arg(command) {
        let path = if path.is_absolute() { path } else { a.cwd.join(path) };
        return path.exists().then_some(path);
    }
    let id = pi_session_id(command)?;
    find_pi_session_by_id(root, &a.cwd, &id)
}

fn pi_flag_value<'a>(command: &'a str, flag: &str, short: &str) -> Option<&'a str> {
    let mut it = command.split_whitespace();
    while let Some(arg) = it.next() {
        if arg == flag || arg == short {
            return it.next();
        }
        if let Some(v) = arg.strip_prefix(&format!("{flag}=")) {
            return Some(v);
        }
    }
    None
}

fn pi_session_arg(command: &str) -> Option<PathBuf> {
    let v = pi_flag_value(command, "--session", "-s")?;
    (v.ends_with(".jsonl") || v.contains(['/', '\\'])).then(|| PathBuf::from(v))
}

fn pi_session_id(command: &str) -> Option<String> {
    if let Some(id) = pi_flag_value(command, "--session-id", "") {
        return Some(id.to_string());
    }
    let v = pi_flag_value(command, "--session", "-s")?;
    (!v.ends_with(".jsonl") && !v.contains(['/', '\\']) && !v.is_empty()).then(|| v.to_string())
}

fn find_pi_session_by_id(root: &Path, cwd: &Path, id: &str) -> Option<PathBuf> {
    let dir = pi_bucket(&cwd.to_string_lossy());
    std::fs::read_dir(root.join(dir))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .find_map(|path| {
            let mut head = String::new();
            File::open(&path).ok()?.take(8192).read_to_string(&mut head).ok()?;
            let v: Value = serde_json::from_str(head.lines().next()?).ok()?;
            (v["id"].as_str() == Some(id)).then_some(path)
        })
}

fn codex_session(a: &mut ExternalAgent, root: &Path, own: &HashSet<String>) {
    let index_file = root.join("session_index.jsonl");
    let Ok(content) = std::fs::read_to_string(index_file) else {
        return;
    };
    for line in content.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line)
            && let Some(id) = v["id"].as_str()
            && !own.contains(id)
        {
            a.session = id.to_string();
            if let Some(name) = v["thread_name"].as_str() {
                a.title = name.to_string();
            }
            break;
        }
    }
}

fn antigravity_session(a: &mut ExternalAgent, root: &Path, own: &HashSet<String>) {
    let brain_dir = root.join("brain");
    let Ok(entries) = std::fs::read_dir(brain_dir) else {
        return;
    };
    let newest = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|ft| ft.is_dir()))
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            if own.contains(&id) {
                return None;
            }
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((mtime, id))
        })
        .max_by_key(|(mtime, _)| *mtime);
    if let Some((_, id)) = newest {
        a.session = id;
    }
}

fn tail(path: &Path, bytes: u64) -> String {
    let mut buf = Vec::new();
    if let Ok(mut f) = File::open(path) {
        let len = f.metadata().map_or(0, |m| m.len());
        let _ = f.seek(SeekFrom::Start(len.saturating_sub(bytes)));
        let _ = f.take(bytes).read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn opencode_sessions() -> Option<Vec<OcSession>> {
    let api = opencode::service_api()?;
    let get = |path: &str| match api.send(Method::GET, path, None, Duration::from_secs(2)) {
        Ok((200, body)) => Some(body),
        _ => None,
    };
    Some(opencode_states(
        &get("/api/session?limit=50")?,
        &get("/api/session/active")?,
        &get("/api/permission/request")?,
    ))
}

fn token(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    s.split_at(end)
}

fn parse_ps(out: &str) -> Vec<PsRow> {
    out.lines()
        .filter_map(|line| {
            let (pid, rest) = token(line);
            let (ppid, rest) = token(rest);
            let (etime, rest) = token(rest);
            let lstart = rest.trim_start();
            let command = (0..5).fold(lstart, |s, _| token(s).1);
            Some(PsRow {
                pid: pid.parse().ok()?,
                ppid: ppid.parse().ok()?,
                etime: etime.into(),
                started: lstart[..lstart.len() - command.len()].into(),
                command: Some(command.trim()).filter(|c| !c.is_empty())?.into(),
            })
        })
        .collect()
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn agent_rows(rows: &[PsRow], own_pid: u32) -> Vec<(Provider, PsRow, String)> {
    let by_pid: HashMap<u32, &PsRow> = rows.iter().map(|r| (r.pid, r)).collect();
    let owned = |pid| {
        std::iter::successors(Some(pid), |p| by_pid.get(p).map(|r| r.ppid).filter(|pp| pp != p))
            .take(64)
            .any(|p| p == own_pid)
    };
    rows.iter()
        .filter_map(|r| {
            let args: Vec<&str> = r.command.split_whitespace().collect();
            let provider = match basename(args.first()?) {
                "claude" => Provider::Claude,
                "codex" => Provider::Codex,
                "pi" => Provider::Pi,
                "node" if r.command.contains("pi-coding-agent") => Provider::Pi,
                "opencode" => Provider::OpenCode,
                "agy" | "antigravity" => Provider::Antigravity,
                _ => return None,
            };
            let helper = matches!(args.get(1).copied(), Some("serve" | "app-server"))
                || args.contains(&"--chrome-native-host");
            let headless = args.iter().any(|a| matches!(*a, "-p" | "--print" | "--mode=rpc"))
                || args.windows(2).any(|w| w == ["--mode", "rpc"]);
            if helper || headless || owned(r.pid) {
                return None;
            }
            let origin = by_pid.get(&r.ppid).map(|p| origin(&p.command)).unwrap_or_default();
            Some((provider, r.clone(), origin))
        })
        .collect()
}

fn origin(parent: &str) -> String {
    match basename(parent.split_whitespace().next().unwrap_or_default()).trim_start_matches('-') {
        "zsh" | "bash" | "fish" | "sh" | "login" => "Terminal".into(),
        "tmux" => "tmux".into(),
        name => match parent.split_once(".app/") {
            Some((app, _)) => basename(app).into(),
            None => name.into(),
        },
    }
}

fn parse_lsof_cwd(out: &str) -> HashMap<u32, PathBuf> {
    let mut pid = 0;
    out.lines()
        .filter_map(|line| match line.split_at_checked(1)? {
            ("p", p) => {
                pid = p.parse().unwrap_or(0);
                None
            }
            ("n", path) => Some((pid, PathBuf::from(path))),
            _ => None,
        })
        .collect()
}

fn claude_entry(
    json: &str,
    pid: u32,
) -> Option<(String, PathBuf, String, ExternalState, Option<String>)> {
    let v: Value = serde_json::from_str(json).ok()?;
    if v["pid"] != pid {
        return None;
    }
    let text = |k: &str| v[k].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    let state = match v["status"].as_str() {
        Some("busy") => Working,
        Some("waiting") => Waiting(text("waitingFor").unwrap_or_else(|| "input needed".into())),
        _ => Idle,
    };
    Some((
        text("sessionId")?,
        text("cwd").unwrap_or_default().into(),
        text("name").unwrap_or_default(),
        state,
        text("tmux"),
    ))
}

fn pi_state(tail: &str, quiet_secs: u64) -> ExternalState {
    let last = tail
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v["type"] == "message");
    let done = last.is_some_and(|v| {
        let m = &v["message"];
        m["role"] == "assistant"
            && matches!(m["stopReason"].as_str(), Some("stop" | "error" | "aborted" | "length"))
    });
    if done && quiet_secs > 5 { Idle } else { Working }
}

fn opencode_states(sessions: &str, active: &str, permissions: &str) -> Vec<OcSession> {
    let parse = |s: &str| serde_json::from_str::<Value>(s).unwrap_or_default();
    let (sessions, active, permissions) = (parse(sessions), parse(active), parse(permissions));
    let asking: HashSet<&str> = permissions["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p["sessionID"].as_str())
        .collect();
    let mut list: Vec<&Value> = sessions["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["parentID"].is_null())
        .collect();
    // newest first: merge gives each TUI the first match in its dir
    list.sort_by_key(|s| std::cmp::Reverse(s["time"]["updated"].as_u64()));
    list.into_iter()
        .filter_map(|s| {
            let id = s["id"].as_str()?;
            let state = if asking.contains(id) {
                Waiting("approval".into())
            } else if active["data"].get(id).is_some() {
                Working
            } else {
                Idle
            };
            let title = s["title"]
                .as_str()
                .filter(|t| !opencode::is_placeholder_title(t))
                .unwrap_or_default();
            let dir = s["location"]["directory"].as_str().unwrap_or_default();
            Some((id.into(), dir.into(), title.into(), state, s["time"]["updated"].as_u64()))
        })
        .collect()
}

fn etime_secs(etime: &str) -> u64 {
    let (days, hms) = etime.split_once('-').unwrap_or(("0", etime));
    let secs = hms.split(':').fold(0, |acc, n| acc * 60 + n.parse::<u64>().unwrap_or(0));
    days.parse::<u64>().unwrap_or(0) * 86_400 + secs
}

/// Joins OpenCode TUIs to their sessions (`None`: service unreachable), adds
/// busy sessions with no TUI, fills titles, and drops aether's own OpenCode sessions.
fn merge(
    mut agents: Vec<ExternalAgent>,
    opencode: Option<Vec<OcSession>>,
    own: &HashSet<String>,
    now_ms: u64,
) -> Vec<ExternalAgent> {
    if let Some(mut sessions) = opencode {
        sessions.retain(|s| !own.contains(&s.0));
        for a in agents.iter_mut().filter(|a| a.provider == Provider::OpenCode) {
            a.exact = true;
            let start_ms = (!a.elapsed.is_empty())
                .then(|| now_ms.saturating_sub((etime_secs(&a.elapsed) + 1) * 1000));
            let fresh =
                |s: &OcSession| start_ms.zip(s.4).is_none_or(|(start, updated)| updated >= start);
            if let Some(i) = sessions
                .iter()
                .position(|s| !a.cwd.as_os_str().is_empty() && s.1 == a.cwd && fresh(s))
            {
                (a.session, _, a.title, a.state, _) = sessions.remove(i);
            }
        }
        agents.extend(sessions.into_iter().filter(|s| s.3 != Idle).map(
            |(session, cwd, title, state, _)| ExternalAgent {
                provider: Provider::OpenCode,
                pid: None,
                started: String::new(),
                elapsed: String::new(),
                cwd,
                session,
                title,
                state,
                exact: true,
                origin: "OpenCode service".into(),
            },
        ));
    }
    for a in &mut agents {
        if a.title.is_empty() {
            a.title =
                a.cwd.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        }
    }
    agents
}

/// Pure pre-signal check. `Ok(None)`: no signal, use the OpenCode API.
fn signal_plan(
    a: &ExternalAgent,
    force: bool,
    lstart: Option<&str>,
    command: Option<&str>,
) -> Result<Option<&'static str>, String> {
    let name_ok = |c: &str| {
        let first = c.split_whitespace().next().map_or("", basename);
        match a.provider {
            Provider::Claude => first == "claude",
            Provider::Codex => first == "codex",
            Provider::Antigravity => first == "agy" || first == "antigravity",
            _ => first == "pi" || (first == "node" && c.contains("pi-coding-agent")),
        }
    };
    match a.provider {
        Provider::OpenCode if valid_session_id(&a.session) => return Ok(None),
        Provider::OpenCode => return Err("no session to stop".into()),
        Provider::Claude | Provider::Pi | Provider::Codex | Provider::Antigravity => {}
        _ => return Err("unsupported provider".into()),
    }
    let same = a.pid.is_some()
        && lstart.is_some_and(|l| l.trim() == a.started.trim())
        && command.is_some_and(name_ok);
    if !same {
        return Err("process changed".into());
    }
    Ok(Some(if (a.provider == Provider::Claude || a.provider == Provider::Codex) && !force {
        "INT"
    } else {
        "TERM"
    }))
}

/// `ps -o lstart= -o command=` output to (lstart, command); lstart is a fixed 24 chars.
fn split_ps_identity(out: &str) -> (Option<&str>, Option<&str>) {
    let out = out.trim();
    match out.split_at_checked(24) {
        Some((l, c)) if !c.trim().is_empty() => (Some(l), Some(c.trim())),
        _ => (None, None),
    }
}

fn valid_session_id(id: &str) -> bool {
    id.starts_with("ses_") && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn read_lossy(path: impl AsRef<Path>) -> Option<String> {
    Some(String::from_utf8_lossy(&std::fs::read(path).ok()?).into_owned())
}

/// Blocking. Never SIGKILL.
pub fn stop(a: &ExternalAgent, force: bool) -> Result<(), String> {
    if a.provider != Provider::OpenCode {
        let pid = a.pid.filter(|p| *p > 1 && *p != std::process::id()).ok_or("process changed")?;
        // re-read right before signalling; lstart is a fixed 24 chars
        let ps = run("/bin/ps", &["-p", &pid.to_string(), "-o", "lstart=", "-o", "command="])
            .unwrap_or_default();
        let (lstart, command) = split_ps_identity(&ps);
        let sig = signal_plan(a, force, lstart, command)?.ok_or("no signal")?;
        let ok = Command::new("/bin/kill")
            .args([format!("-{sig}"), pid.to_string()])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        return if ok { Ok(()) } else { Err("signal failed".into()) };
    }
    signal_plan(a, force, None, None)?;
    let api = opencode::service_api().ok_or("opencode service unreachable")?;
    let path = format!("/api/session/{}/interrupt?resume=false", a.session);
    match api.send(Method::POST, &path, None, Duration::from_secs(3))? {
        (200..=299, _) => Ok(()),
        (status, _) => Err(format!("interrupt returned {status}")),
    }
}

/// User/assistant text, oldest first, last 200.
pub fn transcript(a: &ExternalAgent) -> Vec<(Role, String)> {
    let mut out = match a.provider {
        Provider::Claude => {
            claude_file(&a.session).map(|t| claude_transcript(&t)).unwrap_or_default()
        }
        Provider::Codex if !a.session.is_empty() => {
            codex_file(&home_dir().join(".codex"), &a.session)
                .and_then(read_lossy)
                .map(|t| codex_transcript(&t))
                .unwrap_or_default()
        }
        Provider::Pi if a.session.ends_with(".jsonl") => {
            read_lossy(&a.session).map(|t| pi_transcript(&t)).unwrap_or_default()
        }
        Provider::OpenCode if valid_session_id(&a.session) => opencode::service_api()
            .and_then(|api| {
                let path = format!("/api/session/{}/message?order=desc&limit=200", a.session);
                match api.send(Method::GET, &path, None, Duration::from_secs(3)) {
                    Ok((200, body)) => Some(opencode_transcript(&body)),
                    _ => None,
                }
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    out.drain(..out.len().saturating_sub(200));
    out
}

fn claude_file(session: &str) -> Option<String> {
    if session.is_empty() || session.contains(['/', '\\']) {
        return None;
    }
    let name = format!("{session}.jsonl");
    std::fs::read_dir(claude::config_dir().join("projects"))
        .ok()?
        .flatten()
        .find_map(|d| read_lossy(d.path().join(&name)))
}

fn role_of(s: &str) -> Option<Role> {
    match s {
        "user" => Some(Role::User),
        "assistant" => Some(Role::Assistant),
        _ => None,
    }
}

/// Plain string, or the text blocks of a content array joined by newlines.
fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.trim().to_string(),
        Value::Array(parts) => parts
            .iter()
            .filter(|p| p["type"] == "text")
            .filter_map(|p| p["text"].as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn claude_transcript(jsonl: &str) -> Vec<(Role, String)> {
    jsonl
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| {
            v["isMeta"] != true && v["isSidechain"] != true && v["isCompactSummary"] != true
        })
        .filter_map(|v| {
            let text = text_of(&v["message"]["content"]);
            let echo = text.starts_with("<command-name>") || text.starts_with("<local-command-");
            Some((role_of(v["type"].as_str()?)?, text)).filter(|(_, t)| !t.is_empty() && !echo)
        })
        .collect()
}

fn codex_file(root: &Path, session: &str) -> Option<String> {
    if session.is_empty() || session.contains(['/', '\\']) {
        return None;
    }
    let suffix = format!("-{session}.jsonl");
    let base = root.join("sessions");
    for year in std::fs::read_dir(&base).ok()?.flatten() {
        if !year.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        for month in std::fs::read_dir(year.path()).ok()?.flatten() {
            if !month.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            for day in std::fs::read_dir(month.path()).ok()?.flatten() {
                if !day.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                for file in std::fs::read_dir(day.path()).ok()?.flatten() {
                    if file.file_name().to_string_lossy().ends_with(&suffix) {
                        return Some(file.path().to_string_lossy().into_owned());
                    }
                }
            }
        }
    }
    None
}

fn codex_transcript(jsonl: &str) -> Vec<(Role, String)> {
    jsonl
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| {
            let p = &v["payload"];
            if v["type"] != "response_item" || p["type"] != "message" {
                return None;
            }
            let text = p["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|c| {
                    matches!(c["type"].as_str(), Some("input_text" | "output_text" | "text"))
                })
                .filter_map(|c| c["text"].as_str())
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .filter(|t| {
                    !["<environment_context>", "<user_instructions>"]
                        .iter()
                        .any(|tag| t.starts_with(tag))
                })
                .collect::<Vec<_>>()
                .join("\n");
            Some((role_of(p["role"].as_str()?)?, text)).filter(|(_, t)| !t.is_empty())
        })
        .collect()
}

fn pi_transcript(jsonl: &str) -> Vec<(Role, String)> {
    jsonl
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["type"] == "message")
        .filter_map(|v| {
            let m = &v["message"];
            let text = text_of(&m["content"]);
            Some((role_of(m["role"].as_str()?)?, text)).filter(|(_, t)| !t.is_empty())
        })
        .collect()
}

// user messages may carry `text` directly; assistant ones carry `content[]`
// newest first on the wire (`order=desc`); returned oldest first
fn opencode_transcript(body: &str) -> Vec<(Role, String)> {
    let v: Value = serde_json::from_str(body).unwrap_or_default();
    let mut out: Vec<_> = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let role = role_of(m["type"].as_str().or_else(|| m["role"].as_str())?)?;
            let text = match m["text"].as_str() {
                Some(t) => t.trim().to_string(),
                None => text_of(&m["content"]),
            };
            Some((role, text)).filter(|(_, t)| !t.is_empty())
        })
        .collect();
    out.reverse();
    out
}

#[cfg(test)]
#[path = "../tests/unit/external.rs"]
mod tests;
