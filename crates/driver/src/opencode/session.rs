use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::channel::mpsc::UnboundedSender;
use futures::channel::oneshot;
use futures::future::{Either, select};
use reqwest::Method;
use serde_json::{Value, json};

use aether_core::{
    AccessMode, Attachment, MutexExt, Notice, NoticeKind, NoticeState, ToolCall, ToolDetail,
    ToolStatus, Update,
};

use super::events::{disconnected, is_placeholder_title, map_event, same_file};
use super::service::{
    Api, MCP_WAIT, REQUEST_TIMEOUT, connect, mcp_ready, read_service, service_log_tail,
};
use super::{Child, Conn, State};
use crate::DriverStartOptions;
use crate::record::Recorder;
use crate::tool::{Throttle, start as start_call};

pub(super) const RECONNECT_DEADLINE: Duration = Duration::from_secs(30);

pub(super) fn start(
    opts: &DriverStartOptions,
    state: &Arc<Mutex<State>>,
    tx: &UnboundedSender<Update>,
    rec: &Option<Arc<Recorder>>,
) -> Result<Conn, String> {
    let (api, server) = connect(opts, tx)?;
    let api = Arc::new(api);
    let resume = opts.resume.starts_with("ses_");
    let session = if resume { opts.resume.clone() } else { new_session_id() };
    {
        let mut s = state.guard();
        s.root = session.clone();
        s.model = model_ref(&opts.model, &opts.effort);
    }
    let started = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let events = api.events()?;
    let (stop, stopped) = oneshot::channel();
    {
        let (api, session, state, tx) = (api.clone(), session.clone(), state.clone(), tx.clone());
        let rec = rec.clone();
        std::thread::spawn(move || listen(api, session, events, state, tx, rec, stopped));
    }
    let c = Conn { api, session, resumed: false, ephemeral: opts.ephemeral, _stop: stop, server };
    let label = if resume { "Resuming session" } else { "Creating session" };
    step(tx, NoticeState::Running, label);
    let setup = || -> Result<Option<Value>, String> {
        let info = if resume {
            match c.api.send(Method::GET, &c.path(""), None, REQUEST_TIMEOUT)? {
                (200, text) => Some(serde_json::from_str::<Value>(&text).unwrap_or_default()),
                (404, _) => None,
                (status, text) => {
                    return Err(format!("opencode GET {} returned {status}: {text}", c.path("")));
                }
            }
        } else {
            None
        };
        if info.is_none() {
            c.api.call(Method::POST, "/api/session", Some(create_body(&c.session, opts)))?;
        } else {
            let rules = json!({"permissions": rules(opts.access)});
            c.api.call(Method::PATCH, &c.path(""), Some(rules))?;
            if let Some(model) = model_ref(&opts.model, &opts.effort) {
                let _ = c.api.call(Method::POST, &c.path("/model"), Some(json!({"model": model})));
            }
        }
        Ok(info)
    };
    let info = setup().inspect_err(|_| step(tx, NoticeState::Failed, label))?;
    let resumed = info.is_some();
    if let Some(notice) = lost_resume(resume, resumed) {
        let _ = tx.unbounded_send(Update::Notice(notice));
    }
    step(tx, NoticeState::Done, label);
    let _ = tx.unbounded_send(Update::Mode(opts.access));
    let name = served_model(info.as_ref().map(|i| &i["data"]["model"]), &mut state.guard().model);
    if let Some(name) = name {
        let _ = tx.unbounded_send(Update::Model(name));
    }
    let _ = tx.unbounded_send(Update::Session(c.session.clone()));
    if let Some(title) = info.as_ref().and_then(|i| i["data"]["title"].as_str())
        && !is_placeholder_title(title)
    {
        let _ = tx.unbounded_send(Update::Title(title.into()));
    }
    if resumed {
        reattach(&c, state, tx, rec, started, opts.reattach);
    }
    if c.server.is_some() {
        let mcp = located("/api/mcp", &location_dir(opts).to_string_lossy());
        if !mcp_ready(|| c.api.call(Method::GET, &mcp, None).ok(), MCP_WAIT) {
            eprintln!("aether: opencode lead tools not ready");
        }
    }
    Ok(Conn { resumed, ..c })
}

/// The model in use as `provider/id`; the seeded one (from the options) wins over the stale session info.
pub(super) fn served_model(info: Option<&Value>, seeded: &mut Option<Value>) -> Option<String> {
    if seeded.is_none()
        && let Some(m) = info.filter(|m| m["id"].is_string())
    {
        *seeded = Some(json!({"providerID": m["providerID"], "id": m["id"]}));
    }
    let m = seeded.as_ref()?;
    Some(format!("{}/{}", m["providerID"].as_str()?, m["id"].as_str()?))
}

pub(super) fn lost_resume(requested: bool, resumed: bool) -> Option<Notice> {
    if requested && !resumed {
        Some(Notice::new(
            "info-resume",
            NoticeKind::Info,
            "Couldn't find the earlier OpenCode session, so this is a new one.",
        ))
    } else {
        None
    }
}

fn step(tx: &UnboundedSender<Update>, state: NoticeState, text: &str) {
    let notice = Notice { state, ..Notice::new("startup-session", NoticeKind::Startup, text) };
    let _ = tx.unbounded_send(Update::Notice(notice));
}

/// Picks up a turn the service kept running while Aether was closed; with `settle`,
/// closes out the unfinished reply of one that ended meanwhile. Pending asks show either way.
fn reattach(
    c: &Conn,
    state: &Mutex<State>,
    tx: &UnboundedSender<Update>,
    rec: &Option<Arc<Recorder>>,
    started: u64,
    settle: bool,
) {
    let notice = |kind, text: &str| {
        Update::Notice(Notice {
            id: "reattach".into(),
            kind,
            text: text.into(),
            state: NoticeState::Done,
            until_ms: None,
            url: None,
            detail: None,
        })
    };
    let mut restoring = false;
    let messages = recent_messages(&c.api, &c.session);
    // locked until the Wake is out: an end event handled before then would never close the turn
    let mut s = state.guard();
    let Ok(active) = c.api.call(Method::GET, "/api/session/active", None) else { return };
    for u in relink(&mut s, &messages, Some(&active), |w| outcome(&c.api, &c.session, w)) {
        let _ = tx.unbounded_send(u);
    }
    let children_run = !s.background.is_empty();
    if active["data"].get(&c.session).is_none() {
        if settle {
            let _ = tx.unbounded_send(Update::Wake);
            if c.server.is_none() {
                let text = "OpenCode finished this turn while Aether was closed";
                let _ = tx.unbounded_send(notice(NoticeKind::Info, text));
            }
            let _ = tx.unbounded_send(Update::Final(String::new()));
        }
    } else {
        s.active = true;
        s.started = started;
        restoring = true;
        let _ = tx.unbounded_send(Update::Wake);
        let _ =
            tx.unbounded_send(notice(NoticeKind::Wake, "Reconnected to a running OpenCode turn"));
    }
    drop(s);
    if restoring || children_run {
        restore_children(&c.api, &c.session, state);
    }
    let send = |u| drop(tx.unbounded_send(u));
    for (ty, data) in resync(&c.api, &c.session, true, started, &mut HashMap::new()) {
        if !apply(&c.api, state, &send, rec, &ty, &data) {
            return;
        }
    }
}

/// Maps one event into updates and approves what access allows; false once the session died.
fn apply(
    api: &Api,
    state: &Mutex<State>,
    send: &dyn Fn(Update),
    rec: &Option<Arc<Recorder>>,
    ty: &str,
    data: &Value,
) -> bool {
    if let Some(rec) = rec {
        rec.line(&json!({"type": ty, "data": data}).to_string());
    }
    let mut s = state.guard();
    if s.dead.is_some() {
        return false;
    }
    for u in map_event(&mut s, ty, data) {
        send(u);
    }
    let auto = std::mem::take(&mut s.auto);
    drop(s);
    for (request, asker) in auto {
        if let Err(e) = reply(api, &asker, &request, "once", None) {
            send(Update::Warning(e));
        }
    }
    true
}

fn listen(
    api: Arc<Api>,
    session: String,
    mut res: reqwest::Response,
    state: Arc<Mutex<State>>,
    tx: UnboundedSender<Update>,
    rec: Option<Arc<Recorder>>,
    mut stop: oneshot::Receiver<()>,
) {
    let throttle = Throttle::new(tx.clone());
    let mut family = HashMap::new();
    let send = |u| throttle.send(u);
    let handle = |ty: &str, data: &Value| apply(&api, &state, &send, &rec, ty, data);
    loop {
        let mut buf = Vec::new();
        loop {
            let chunk = match api.rt.block_on(select(&mut stop, Box::pin(res.chunk()))) {
                Either::Left(_) => return,
                Either::Right((Ok(Some(chunk)), _)) => chunk,
                Either::Right(_) => break,
            };
            buf.extend_from_slice(&chunk);
            for v in frames(&mut buf) {
                let data = &v["data"];
                let ty = v["type"].as_str().unwrap_or_default();
                // form.created carries its session only inside the form
                let sid = data["sessionID"].as_str().or(data["form"]["sessionID"].as_str());
                let Some(sid) = sid else { continue };
                if sid != session
                    && !state.guard().children.contains_key(sid)
                    && !(ASK_EVENTS.contains(&ty) && is_family(&api, &session, sid, &mut family))
                {
                    continue;
                }
                if ty == "session.execution.succeeded" && sid == session {
                    match command_changes(&api, &session, &state) {
                        Ok(Some(diff)) if !handle("aether.diff", &diff) => return,
                        Err(e) => send(Update::Warning(e)),
                        _ => {}
                    }
                }
                if !handle(ty, data) {
                    return;
                }
            }
        }
        let dropped = Instant::now();
        res = loop {
            std::thread::sleep(Duration::from_secs(1));
            if !matches!(stop.try_recv(), Ok(None)) {
                return;
            }
            if !api.dedicated
                && let Some(fresh) = read_service()
            {
                *api.service.guard() = fresh;
            }
            if let Ok(res) = api.events() {
                state.guard().lost = false;
                break res;
            }
            if dropped.elapsed() > RECONNECT_DEADLINE {
                let mut s = state.guard();
                if !s.lost {
                    let lost = disconnected(&mut s, || service_log_tail(20));
                    drop(s);
                    if !lost.is_empty() {
                        lost.into_iter().for_each(&send);
                        return;
                    }
                }
            }
        };
        let messages = recent_messages(&api, &session);
        let running = api.call(Method::GET, "/api/session/active", None).ok();
        let (active, started, children_run) = {
            let mut s = state.guard();
            let ended = |w: &Work| outcome(&api, &session, w);
            relink(&mut s, &messages, running.as_ref(), ended).into_iter().for_each(&send);
            (s.active, s.started, !s.background.is_empty())
        };
        if active || children_run {
            restore_children(&api, &session, &state);
        }
        for (ty, data) in resync(&api, &session, active, started, &mut family) {
            if !handle(&ty, &data) {
                return;
            }
        }
    }
}

/// The turn's changed files no Edit row covers, with their patches from the session diff.
fn command_changes(
    api: &Api,
    session: &str,
    state: &Mutex<State>,
) -> Result<Option<Value>, String> {
    {
        let s = state.guard();
        let uncovered = |f: &String| !s.edited.iter().any(|e| same_file(e, f));
        if !s.changed.iter().any(uncovered) {
            return Ok(None);
        }
    }
    let v = api.call(Method::GET, &format!("/api/session/{session}/diff"), None)?;
    let files = v["data"].as_array().or(v["data"]["files"].as_array());
    Ok(files.map(|files| json!({"files": files})))
}

/// Registers the still-running subagent sessions, so their frames are not dropped.
fn restore_children(api: &Api, root: &str, state: &Mutex<State>) {
    let info = api.call(Method::GET, &format!("/api/session/{root}"), None).unwrap_or_default();
    let dir = info["data"]["location"]["directory"].as_str();
    let found = list_children(root, |parent| {
        let tail = children_path(parent, dir);
        let v = api.call(Method::GET, &tail, None).map_err(drop)?;
        let items = v["data"].as_array().cloned().unwrap_or_default();
        Ok(items
            .iter()
            .filter_map(|i| {
                let agent = i["agent"].as_str().unwrap_or_default();
                Some((i["id"].as_str()?.to_string(), agent.to_string()))
            })
            .collect())
    });
    let mut s = state.guard();
    for (id, agent) in found {
        s.children.entry(id.clone()).or_insert(Child { task: id, agent });
    }
}

/// Every descendant session of `root` as (id, agent), up to 8 levels deep; a failed
/// listing skips that branch.
pub(super) fn list_children(
    root: &str,
    mut list: impl FnMut(&str) -> Result<Vec<(String, String)>, ()>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut level = vec![root.to_string()];
    for _ in 0..8 {
        let mut next = Vec::new();
        for parent in &level {
            for (id, agent) in list(parent).unwrap_or_default() {
                if id != root && !out.iter().any(|(seen, _)| *seen == id) {
                    next.push(id.clone());
                    out.push((id, agent));
                }
            }
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }
    out
}

/// Events missed while the stream was down, rebuilt from the session's state.
fn resync(
    api: &Api,
    session: &str,
    active: bool,
    started: u64,
    family: &mut HashMap<String, bool>,
) -> Vec<(String, Value)> {
    let path = format!("/api/session/{session}");
    let info = match api.send(Method::GET, &path, None, REQUEST_TIMEOUT) {
        Ok((404, _)) => return vec![("session.deleted".into(), Value::Null)],
        Ok((200, text)) => serde_json::from_str::<Value>(&text).unwrap_or_default()["data"].take(),
        _ => return vec![],
    };
    let mut out = vec![("session.renamed".into(), info.clone())];
    // a background child may ask while the root is idle
    let mut list = |tail: &str| {
        let v = api.call(Method::GET, tail, None).unwrap_or_default();
        let items = v["data"].as_array().cloned().unwrap_or_default();
        items
            .into_iter()
            .filter(|i| {
                i["sessionID"]
                    .as_str()
                    .is_some_and(|sid| sid == session || is_family(api, session, sid, family))
            })
            .collect::<Vec<_>>()
    };
    let at = |path: &str| match info["location"]["directory"].as_str() {
        Some(dir) => located(path, dir),
        None => format!(
            "/api/session/{session}{}",
            path.trim_start_matches("/api").trim_end_matches("/request")
        ),
    };
    out.extend(
        list(&at("/api/permission/request")).into_iter().map(|p| ("permission.asked".into(), p)),
    );
    out.extend(
        list(&at("/api/form")).into_iter().map(|f| ("form.created".into(), json!({"form": f}))),
    );
    if !active {
        return out;
    }
    let running = api
        .call(Method::GET, "/api/session/active", None)
        .is_ok_and(|v| v["data"].get(session).is_some());
    // a shutdown suspends the run without an idle mark; the service resumes it on boot
    if !running && info["time"]["idle"].as_f64().is_some_and(|idle| idle >= started as f64) {
        let outcome = info["outcome"].as_str().unwrap_or("succeeded");
        out.push((format!("session.execution.{outcome}"), json!({})));
    }
    out
}

/// What a Background row runs on.
#[derive(Debug)]
pub(super) enum Work {
    Child(String),
    Shell(String),
}

/// How `work` ended: `None` while it still runs or the service can't say.
fn outcome(api: &Api, root: &str, work: &Work) -> Option<ToolStatus> {
    let ended = |ok: bool| Some(if ok { ToolStatus::Done } else { ToolStatus::Stopped });
    match work {
        Work::Child(sid) => {
            let info = api.call(Method::GET, &format!("/api/session/{sid}"), None);
            ended(info.is_ok_and(|v| v["data"]["outcome"] == "succeeded"))
        }
        Work::Shell(id) => {
            let info = api.call(Method::GET, &format!("/api/session/{root}"), None).ok()?;
            let path = located(
                &format!("/api/shell/{id}"),
                info["data"]["location"]["directory"].as_str()?,
            );
            match api.send(Method::GET, &path, None, REQUEST_TIMEOUT).ok()? {
                (404, _) => ended(false),
                (200, text) => {
                    let v: Value = serde_json::from_str(&text).ok()?;
                    let shell = v.get("data").unwrap_or(&v);
                    match shell["status"].as_str()? {
                        "running" => None,
                        status => ended(status == "exited" && shell["exit"] == 0),
                    }
                }
                _ => None,
            }
        }
    }
}

/// Rebuilds call → child links and Background rows for subagents still running, settles
/// known Background rows whose child stopped while nobody listened, and settles the restored
/// reply's Background rows whose work ended.
pub(super) fn relink(
    s: &mut State,
    messages: &Value,
    active: Option<&Value>,
    outcome: impl Fn(&Work) -> Option<ToolStatus>,
) -> Vec<Update> {
    // a failed listing would read as every child having ended
    let Some(active) = active else { return vec![] };
    let live = |sid: &str| active["data"].get(sid).is_some();
    let mut gone: Vec<String> = s
        .children
        .iter()
        .filter(|(sid, c)| !live(sid) && s.background.contains_key(&c.task))
        .map(|(_, c)| c.task.clone())
        .collect();
    gone.sort();
    gone.dedup();
    let mut out: Vec<Update> = gone
        .iter()
        .filter_map(|task| s.background.remove(task))
        .map(|mut row| {
            row.status = ToolStatus::Done;
            Update::Tool(row)
        })
        .collect();
    for (child, row) in running_tasks(messages).into_iter().filter(|(c, _)| live(c)) {
        let agent = match &row.detail {
            ToolDetail::Task { agent, .. } => agent.clone(),
            _ => String::new(),
        };
        s.children.insert(child, Child { task: row.id.clone(), agent });
        s.background.entry(row.id.clone()).or_insert(row);
    }
    for id in std::mem::take(&mut s.restored) {
        if s.background.contains_key(&id) {
            continue;
        }
        let Some((work, mut row)) = background_part(messages, &id) else { continue };
        let ended = match &work {
            Work::Child(sid) if live(sid) => None,
            work => outcome(work),
        };
        match (ended, work) {
            (Some(status), _) => {
                row.status = status;
                out.push(Update::Tool(row));
            }
            (None, Work::Shell(shell)) => {
                s.shells.insert(shell, id.clone());
                s.background.insert(id, row);
            }
            (None, Work::Child(_)) => {}
        }
    }
    out
}

fn tool_parts(messages: &Value) -> impl Iterator<Item = &Value> {
    let parts = messages["data"].as_array().into_iter().flatten();
    parts
        .flat_map(|m| m["content"].as_array().into_iter().flatten())
        .filter(|p| p["type"] == "tool")
}

/// Subagent calls the root's messages say still run in the background, as (child session, row).
fn running_tasks(messages: &Value) -> Vec<(String, ToolCall)> {
    tool_parts(messages)
        .filter(|p| p["state"]["metadata"]["status"] == "running")
        .filter_map(|p| {
            let state = &p["state"];
            let child = state["metadata"]["sessionID"].as_str()?;
            let mut row = start_call(p["id"].as_str()?, p["name"].as_str()?, &state["input"]);
            if !matches!(row.detail, ToolDetail::Task { .. }) {
                return None;
            }
            row.status = ToolStatus::Background;
            Some((child.to_string(), row))
        })
        .collect()
}

/// The child session or shell call `id` ran on, with its row.
fn background_part(messages: &Value, id: &str) -> Option<(Work, ToolCall)> {
    let p = tool_parts(messages).find(|p| p["id"] == id)?;
    let meta = &p["state"]["metadata"];
    let work = match (meta["shellID"].as_str(), meta["sessionID"].as_str()) {
        (Some(shell), _) => Work::Shell(shell.into()),
        (None, Some(sid)) => Work::Child(sid.into()),
        _ => return None,
    };
    let mut row = start_call(id, p["name"].as_str().unwrap_or_default(), &p["state"]["input"]);
    row.status = ToolStatus::Background;
    Some((work, row))
}

// ponytail: the newest 20 messages; a background agent spawned further back stays unlinked
fn recent_messages(api: &Api, session: &str) -> Value {
    let path = format!("/api/session/{session}/message?order=desc&limit=20");
    api.call(Method::GET, &path, None).unwrap_or_default()
}

pub(super) fn located(path: &str, dir: &str) -> String {
    format!("{path}?location%5Bdirectory%5D={}", encode(dir))
}

fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub(super) fn children_path(parent: &str, dir: Option<&str>) -> String {
    let path = format!("/api/session?parentID={parent}");
    match dir {
        Some(dir) => format!("{path}&location%5Bdirectory%5D={}", encode(dir)),
        None => path,
    }
}

const ASK_EVENTS: [&str; 6] = [
    "permission.asked",
    "permission.replied",
    "permission.rejected",
    "form.created",
    "form.replied",
    "form.cancelled",
];

/// Whether `sid` is a subagent session of `root`, at any depth up to 8 levels.
pub(super) fn descends(
    root: &str,
    sid: &str,
    cache: &mut HashMap<String, bool>,
    mut parent_of: impl FnMut(&str) -> Result<Option<String>, ()>,
) -> bool {
    let mut path = vec![sid.to_string()];
    let found = loop {
        let cur = path.last().unwrap();
        if let Some(&known) = cache.get(cur) {
            path.pop();
            break known;
        }
        if path.len() > 8 {
            return false;
        }
        match parent_of(cur) {
            Err(()) => return false,
            Ok(Some(p)) if p == root => break true,
            Ok(Some(p)) if !path.contains(&p) => path.push(p),
            Ok(_) => break false,
        }
    };
    for id in path {
        cache.insert(id, found);
    }
    found
}

pub(super) fn retry_lookup(
    attempts: usize,
    pause: Duration,
    mut f: impl FnMut() -> Result<Option<String>, ()>,
) -> Result<Option<String>, ()> {
    let mut out = f();
    for _ in 1..attempts {
        if out.is_ok() {
            break;
        }
        std::thread::sleep(pause);
        out = f();
    }
    out
}

fn is_family(api: &Api, root: &str, sid: &str, cache: &mut HashMap<String, bool>) -> bool {
    let mut failed = false;
    let found = descends(root, sid, cache, |id| {
        let path = format!("/api/session/{id}");
        let out = retry_lookup(3, Duration::from_millis(500), || {
            match api.send(Method::GET, &path, None, Duration::from_secs(5)) {
                Ok((200, text)) => {
                    let v: Value = serde_json::from_str(&text).map_err(drop)?;
                    Ok(v["data"]["parentID"].as_str().map(String::from))
                }
                Ok((404, _)) => Ok(None),
                _ => Err(()),
            }
        });
        failed |= out.is_err();
        out
    });
    if failed && !found {
        eprintln!("aether: opencode ask from {sid} skipped: parent lookup failed");
    }
    found
}

/// Complete SSE frames drained from `buf`, as parsed `data:` payloads.
fn frames(buf: &mut Vec<u8>) -> Vec<Value> {
    let mut out = Vec::new();
    while let Some(end) = buf.windows(2).position(|w| w == b"\n\n") {
        let frame: Vec<u8> = buf.drain(..end + 2).collect();
        let frame = String::from_utf8_lossy(&frame);
        let data: Vec<&str> =
            frame.lines().filter_map(|l| l.strip_prefix("data:")).map(str::trim_start).collect();
        if let Ok(v) = serde_json::from_str(&data.join("\n")) {
            out.push(v);
        }
    }
    out
}

pub(super) fn reply(
    api: &Api,
    session: &str,
    request: &str,
    decision: &str,
    message: Option<&str>,
) -> Result<(), String> {
    let path = format!("/api/session/{session}/permission/{request}/reply");
    let mut body = json!({"decision": decision});
    if let Some(message) = message {
        body["message"] = message.into();
    }
    api.call(Method::POST, &path, Some(body)).map(drop)
}

pub(super) fn reply_form(
    api: &Api,
    session: &str,
    form_id: &str,
    body: &Value,
) -> Result<(), String> {
    let path = format!("/api/session/{session}/form/{form_id}/reply");
    api.call(Method::POST, &path, Some(body.clone())).map(drop)
}

pub(super) fn cancel_form(api: &Api, session: &str, form_id: &str) -> Result<(), String> {
    api.call(Method::DELETE, &format!("/api/session/{session}/form/{form_id}"), None).map(drop)
}

/// Sends `text` as a turn, returning its inbox id (none for a `/command`):
/// `/compact` and registered `/command`s run natively.
pub(super) fn submit(c: &Conn, text: &str, images: &[Attachment]) -> Result<String, String> {
    let inbox = |v: Value| v["data"]["id"].as_str().unwrap_or_default().to_string();
    if let Some((name, args)) = command(text) {
        if name == "compact" {
            return c.api.call(Method::POST, &c.path("/compact"), Some(json!({}))).map(inbox);
        }
        let body = json!({"name": name, "text": args});
        match c.api.send(Method::POST, &c.path("/command"), Some(body), REQUEST_TIMEOUT)? {
            (200..300, _) => return Ok(String::new()),
            (404, text) if text.contains("CommandNotFoundError") => {}
            (status, text) => {
                let text: String = text.chars().take(300).collect();
                return Err(format!("opencode command /{name} returned {status}: {text}"));
            }
        }
    }
    c.api.call(Method::POST, &c.path("/prompt"), Some(prompt_body(text, images, None))).map(inbox)
}

fn file_uri(path: &str) -> String {
    let segments: Vec<String> = path.split('/').map(encode).collect();
    format!("file://{}", segments.join("/"))
}

pub(super) fn prompt_body(text: &str, images: &[Attachment], delivery: Option<&str>) -> Value {
    let mut body = json!({"text": text});
    if !images.is_empty() {
        let files: Vec<Value> =
            images.iter().map(|i| json!({"uri": file_uri(&i.path.to_string_lossy())})).collect();
        body["files"] = files.into();
    }
    if let Some(delivery) = delivery {
        body["delivery"] = delivery.into();
    }
    body
}

fn command(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix('/')?;
    let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    (!name.is_empty()).then_some((name, args.trim()))
}

// format is load-bearing: OpenCode's free-tier gateway rejects ids it could not have minted;
// its per-ms counter starts at 1, so a zero counter (`fff` tail) reads as forged
fn new_session_id() -> String {
    const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let time = !(millis.wrapping_mul(0x1000) + 1) & 0xFFFF_FFFF_FFFF;
    let mut b = [0u8; 14];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    let suffix: String = b.iter().map(|x| ALPHABET[*x as usize % 62] as char).collect();
    format!("ses_{time:012x}{suffix}")
}

fn is_default(model: &str) -> bool {
    model.is_empty()
        || model.eq_ignore_ascii_case("default")
        || model.eq_ignore_ascii_case("opencode")
}

pub(super) fn model_ref(model: &str, effort: &str) -> Option<Value> {
    let trimmed = model.trim();
    if is_default(trimmed) {
        return None;
    }
    let (provider, id) =
        trimmed.split_once('/').filter(|(p, id)| !p.is_empty() && !id.is_empty())?;
    let mut model = json!({"providerID": provider, "id": id});
    if !effort.is_empty() {
        model["variant"] = effort.into();
    }
    Some(model)
}

/// The `/model` body for a change of model or effort; none when the session already runs it.
/// An empty or default model resets to the server's default.
pub(super) fn model_switch(
    model: &str,
    effort: &str,
    current: Option<&Value>,
    default: &Value,
) -> Option<Value> {
    let named = model_ref(model, "");
    if named.is_none() && !is_default(model.trim()) {
        return None;
    }
    let reset = named.is_none();
    let base = named.as_ref().unwrap_or(default);
    let mut target = json!({"providerID": base["providerID"], "id": base["id"]});
    if target["id"].is_null() || (reset && current.is_none() && effort.is_empty()) {
        return None;
    }
    if !effort.is_empty() {
        target["variant"] = effort.into();
    }
    (current != Some(&target)).then(|| json!({"model": target}))
}

// the server matches directories by exact string; trailing slash or symlink splits sessions
fn location_dir(opts: &DriverStartOptions) -> std::path::PathBuf {
    std::fs::canonicalize(&opts.cwd).unwrap_or_else(|_| opts.cwd.components().collect())
}

fn create_body(id: &str, opts: &DriverStartOptions) -> Value {
    let dir = location_dir(opts);
    let mut body = json!({
        "id": id,
        "location": {"directory": dir.to_string_lossy()},
        "agent": "build",
        "permissions": rules(opts.access),
    });
    if let Some(model) = model_ref(&opts.model, &opts.effort) {
        body["model"] = model;
    }
    body
}

// session rules outrank the build agent's allow-all, verified on 2.0.32
fn rules(access: AccessMode) -> Value {
    let ask = |action: &str| json!({"action": action, "resource": "*", "effect": "ask"});
    let deny = |action: &str| json!({"action": action, "resource": "*", "effect": "deny"});
    let mut rules = match access {
        AccessMode::Ask | AccessMode::Auto => vec![ask("edit"), ask("shell")],
        AccessMode::AutoEdit => vec![ask("shell")],
        AccessMode::ReadOnly => vec![deny("edit"), deny("shell")],
        AccessMode::Full => vec![],
    };
    // opencode asks before touching a folder outside the session's; its `*` spans subfolders
    if let Some(dir) = aether_core::attachments_dir() {
        let resource = dir.join("*").to_string_lossy().into_owned();
        rules
            .push(json!({"action": "external_directory", "resource": resource, "effect": "allow"}));
    }
    rules.into()
}

#[cfg(test)]
#[path = "../../tests/unit/opencode/session.rs"]
mod tests;
