use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use reqwest::Method;
use serde_json::Value;

use aether_core::{
    MutexExt, Notice, NoticeKind, NoticeState, Provider, Update, config_dir, home_dir,
};
use futures::channel::mpsc::UnboundedSender;

use crate::lines::{MAX_LINE, capped_lines};
use crate::spawn::{StderrTail, command, spawn_logged};
use crate::version::gate;
use crate::{DriverStartOptions, LeadTools};

pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
// ponytail: fixed wait, then prompt without tools; poll per prompt if mcp starts slower
pub(super) const MCP_WAIT: Duration = Duration::from_secs(15);

pub(super) struct Service {
    url: String,
    password: String,
}

pub struct Api {
    pub(super) rt: tokio::runtime::Runtime,
    http: reqwest::Client,
    pub(super) service: Mutex<Service>,
    /// A lead's own server, never swapped for the shared one in `service.json`.
    pub(super) dedicated: bool,
}

impl Api {
    /// The service `s` names, if its info route answers within 1 s.
    fn probe(s: Service) -> Option<Self> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
        // an idle pooled socket goes stale while no block_on drives the runtime
        let http = reqwest::Client::builder().pool_max_idle_per_host(0).build().ok()?;
        let api = Self { rt, http, service: Mutex::new(s), dedicated: false };
        let (status, _) = api.send(Method::GET, "/api/info", None, Duration::from_secs(1)).ok()?;
        (status == 200).then_some(api)
    }

    fn dedicated(url: String, password: String) -> Option<Self> {
        Self::probe(Service { url, password }).map(|api| Self { dedicated: true, ..api })
    }

    /// The service's own version, from its info route.
    pub fn version(&self) -> Option<String> {
        let (_, text) = self.send(Method::GET, "/api/info", None, Duration::from_secs(1)).ok()?;
        let v: Value = serde_json::from_str(&text).ok()?;
        v["version"].as_str().map(String::from)
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let s = self.service.guard();
        self.http
            .request(method, format!("{}{path}", s.url))
            .basic_auth("opencode", Some(&s.password))
    }

    pub fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        timeout: Duration,
    ) -> Result<(u16, String), String> {
        let mut req = self.request(method.clone(), path).timeout(timeout);
        if let Some(body) = body {
            req = req.header("content-type", "application/json").body(body.to_string());
        }
        self.rt
            .block_on(async {
                let res = req.send().await?;
                let status = res.status().as_u16();
                Ok((status, res.text().await?))
            })
            .map_err(|e: reqwest::Error| format!("opencode {method} {path} failed: {e}"))
    }

    pub(super) fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        let (status, text) = self.send(method.clone(), path, body, REQUEST_TIMEOUT)?;
        if !(200..300).contains(&status) {
            let text: String = text.chars().take(300).collect();
            return Err(format!("opencode {method} {path} returned {status}: {text}"));
        }
        Ok(serde_json::from_str(&text).unwrap_or_default())
    }

    pub(super) fn events(&self) -> Result<reqwest::Response, String> {
        let req = self.request(Method::GET, "/api/event").header("accept", "text/event-stream");
        let res = self
            .rt
            .block_on(async { tokio::time::timeout(Duration::from_secs(5), req.send()).await })
            .map_err(|_| "opencode event stream timed out".to_string())?
            .map_err(|e| format!("opencode event stream failed: {e}"))?;
        if res.status() != 200 {
            return Err(format!("opencode event stream returned {}", res.status()));
        }
        Ok(res)
    }
}

fn service_paths() -> Vec<PathBuf> {
    let home = home_dir();
    let mut paths = Vec::new();
    if let Some(state) =
        std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()).map(PathBuf::from)
    {
        paths.push(state.join("opencode/service.json"));
    }
    paths.push(home.join(".local/state/opencode/service.json"));
    if let Some(config) =
        std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from)
    {
        paths.push(config.join("opencode/service.json"));
    }
    paths.push(home.join(".config/opencode/service.json"));
    paths.push(home.join(".opencode/service.json"));
    paths
}

fn parse_service(text: &str) -> Option<Service> {
    let v: Value = serde_json::from_str(text).ok()?;
    let password = v["password"].as_str()?.to_string();
    let url = if let Some(url) = v["url"].as_str() {
        url.trim_end_matches('/').to_string()
    } else {
        let host = v["hostname"].as_str().or(v["host"].as_str()).unwrap_or("127.0.0.1");
        let port = v["port"].as_u64().unwrap_or(4096);
        format!("http://{host}:{port}")
    };
    Some(Service { url, password })
}

fn read_service_from(path: &Path) -> Option<Service> {
    parse_service(&std::fs::read_to_string(path).ok()?)
}

pub(super) fn read_service() -> Option<Service> {
    for path in service_paths() {
        if let Some(service) = read_service_from(&path) {
            return Some(service);
        }
    }
    None
}

/// The running local service, authed. Blocking (file read + up to 1 s of HTTP).
pub fn service_api() -> Option<Api> {
    read_service().and_then(Api::probe)
}

/// Blocking (file read + up to 1 s of HTTP): keep off the UI thread when possible.
pub fn service_reachable() -> bool {
    read_service().and_then(Api::probe).is_some()
}

pub(super) fn service_log() -> Option<PathBuf> {
    Some(config_dir()?.join("logs/opencode-service.log"))
}

pub(super) fn tail_of(path: &Path, lines: usize) -> String {
    let mut tail = StderrTail::default();
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].iter().for_each(|l| tail.push(l));
    tail.fatal("").trim_start_matches('\n').to_string()
}

static STARTED: AtomicBool = AtomicBool::new(false);

pub(super) fn service_log_tail(lines: usize) -> String {
    if !STARTED.load(Ordering::SeqCst) {
        return String::new();
    }
    service_log().map(|p| tail_of(&p, lines)).unwrap_or_default()
}

fn log_stdio() -> (Stdio, Stdio) {
    let file = service_log().and_then(|p| {
        std::fs::create_dir_all(p.parent()?).ok()?;
        std::fs::OpenOptions::new().create(true).append(true).open(p).ok()
    });
    match file.and_then(|f| Some((f.try_clone().ok()?, f))) {
        Some((a, b)) => (a.into(), b.into()),
        None => (Stdio::null(), Stdio::null()),
    }
}

fn startup(tx: &UnboundedSender<Update>, state: NoticeState, text: &str) {
    let notice = Notice { state, ..Notice::new("startup-spawn", NoticeKind::Startup, text) };
    let _ = tx.unbounded_send(Update::Notice(notice));
}

// ponytail: crash leaves lead servers running; add a pid file if it bites
static LEAD_PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

pub(super) struct LeadServer(Child);

impl LeadServer {
    fn new(child: Child) -> Self {
        LEAD_PIDS.guard().push(child.id());
        Self(child)
    }
}

impl Drop for LeadServer {
    fn drop(&mut self) {
        let mut pids = LEAD_PIDS.lock().unwrap_or_else(|e| e.into_inner());
        pids.retain(|&p| p != self.0.id());
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Kills every lead chat's own `opencode serve`; for app quit, where drivers never drop.
pub fn kill_lead_servers() {
    let pids = LEAD_PIDS.lock().unwrap_or_else(|e| e.into_inner());
    for pid in pids.iter() {
        aether_core::signal_process(*pid, "KILL", false);
    }
}

pub(super) fn lead_config(lead: &LeadTools) -> String {
    serde_json::json!({"mcp": {"aether": {
        "type": "local",
        "command": [lead.exe, "mcp"],
        "environment": {"AETHER_ORCH_SOCK": lead.sock, "AETHER_ORCH_TOKEN": lead.token},
        "codemode": false,
    }}})
    .to_string()
}

fn serve(cmd: &mut Command, lead: Option<(&LeadTools, &str)>) {
    let Some((lead, password)) = lead else {
        cmd.args(["serve", "--service"]);
        return;
    };
    cmd.args(["serve", "--port", "0", "--hostname", "127.0.0.1"])
        .env("OPENCODE_SERVER_PASSWORD", password)
        .env("OPENCODE_CONFIG_CONTENT", lead_config(lead));
}

fn listening_url(line: &str) -> Option<String> {
    let (_, url) = line.split_once("server listening on ")?;
    Some(url.trim().trim_end_matches('/').to_string())
}

pub(super) fn mcp_ready(mut list: impl FnMut() -> Option<Value>, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let up = list().is_some_and(|v| {
            v["data"].as_array().is_some_and(|servers| {
                servers
                    .iter()
                    .any(|s| s["name"] == "aether" && s["status"]["status"] == "connected")
            })
        });
        if up {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn scan_stdout(out: impl Read, found: &std::sync::mpsc::Sender<String>) {
    let mut out = BufReader::new(out);
    if let Some(url) = capped_lines(&mut out, MAX_LINE).flatten().find_map(|l| listening_url(&l)) {
        let _ = found.send(url);
    }
    let _ = std::io::copy(&mut out, &mut std::io::sink());
}

fn password() -> std::io::Result<String> {
    Ok(aether_core::random_bytes::<16>().iter().map(|x| format!("{x:02x}")).collect())
}

pub(super) fn connect(
    opts: &DriverStartOptions,
    tx: &UnboundedSender<Update>,
) -> Result<(Api, Option<LeadServer>), String> {
    let connected = match &opts.lead {
        Some(lead) => connect_lead(opts, lead, tx).map(|(api, server)| (api, Some(server))),
        None => connect_inner(opts, tx).map(|api| (api, None)),
    };
    let (api, server) = connected.inspect_err(|_| {
        startup(tx, NoticeState::Failed, "Starting OpenCode service");
    })?;
    gate(Provider::OpenCode, api.version().as_deref(), tx)?;
    Ok((api, server))
}

fn connect_lead(
    opts: &DriverStartOptions,
    lead: &LeadTools,
    tx: &UnboundedSender<Update>,
) -> Result<(Api, LeadServer), String> {
    startup(tx, NoticeState::Running, "Starting OpenCode service");
    let mut cmd = command(opts, "opencode").ok_or_else(|| {
        "opencode is not installed; install it or set an explicit binary path".to_string()
    })?;
    let password = password().map_err(|e| format!("opencode lead password failed: {e}"))?;
    serve(&mut cmd, Some((lead, &password)));
    let (_, err) = log_stdio();
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(err);
    STARTED.store(true, Ordering::SeqCst);
    let mut child = spawn_logged(&mut cmd).map_err(|e| format!("opencode failed to start: {e}"))?;
    let stdout = child.stdout.take();
    let server = LeadServer::new(child);
    let (found, url) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let Some(out) = stdout {
            scan_stdout(out, &found);
        }
    });
    let url = url.recv_timeout(Duration::from_secs(20)).map_err(|e| {
        let why = match e {
            RecvTimeoutError::Timeout => "opencode lead server did not start within 20 s",
            RecvTimeoutError::Disconnected => "opencode lead server exited at startup",
        };
        format!("{why}\n{}", service_log_tail(20)).trim_end().to_string()
    })?;
    let api = Api::dedicated(url, password).ok_or("opencode lead server did not answer")?;
    startup(tx, NoticeState::Done, "Service ready");
    Ok((api, server))
}

fn connect_inner(opts: &DriverStartOptions, tx: &UnboundedSender<Update>) -> Result<Api, String> {
    let adopt = || read_service().and_then(Api::probe);
    if let Some(api) = adopt() {
        startup(tx, NoticeState::Done, "Service ready");
        return Ok(api);
    }
    startup(tx, NoticeState::Running, "Starting OpenCode service");
    let mut cmd = command(opts, "opencode").ok_or_else(|| {
        "opencode is not installed; install it or set an explicit binary path".to_string()
    })?;
    STARTED.store(true, Ordering::SeqCst);
    let (out, err) = log_stdio();
    serve(&mut cmd, None);
    cmd.current_dir(home_dir()).stdin(Stdio::null()).stdout(out).stderr(err);
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY")
        && !key.is_empty()
    {
        cmd.env("OPENROUTER_API_KEY", key);
    }
    let mut child = spawn_logged(&mut cmd).map_err(|e| format!("opencode failed to start: {e}"))?;
    // reap only, never kill: the service may also serve the user's own terminal
    std::thread::spawn(move || child.wait());
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(api) = adopt() {
            startup(tx, NoticeState::Done, "Service ready");
            return Ok(api);
        }
    }
    Err("opencode service did not start within 20 s".into())
}

#[cfg(test)]
#[path = "../../tests/unit/opencode/service.rs"]
mod tests;
