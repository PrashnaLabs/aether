use std::collections::{HashSet, VecDeque};
use std::process::{Child, Command, Stdio};
use std::sync::{LazyLock, Mutex};

use aether_core::{MutexExt, find_binary, repaired_path};

use super::DriverStartOptions;
use crate::tool::clean;

const STDERR_TAIL: usize = 40;

pub fn command(opts: &DriverStartOptions, name: &str) -> Option<Command> {
    let bin = find_binary(name, &opts.binary)?;
    let mut cmd = Command::new(bin);
    cmd.current_dir(&opts.cwd);
    cmd.env("PATH", repaired_path());
    for key in
        ["AETHER_ORCH_SOCK", "AETHER_ORCH_TOKEN", "AETHER_ORCH_SPECS", "OPENCODE_CONFIG_CONTENT"]
    {
        cmd.env_remove(key);
    }
    cmd.stderr(Stdio::piped());
    Some(cmd)
}

pub fn spawn_logged(cmd: &mut Command) -> std::io::Result<Child> {
    eprintln!("spawning: {}", describe(cmd));
    cmd.spawn()
}

fn describe(cmd: &Command) -> String {
    let args = cmd.get_args().map(|a| format!("{a:?}"));
    std::iter::once(format!("{:?}", cmd.get_program())).chain(args).collect::<Vec<_>>().join(" ")
}

pub fn visible_stderr(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("error:") || lower.contains("fatal") || lower.contains("no such file")
}

/// The last non-blank stderr lines, ANSI removed, for the error a dead CLI leaves.
#[derive(Default)]
pub(crate) struct StderrTail(VecDeque<String>);

impl StderrTail {
    pub(crate) fn push(&mut self, line: &str) {
        let line = clean(line);
        if line.trim().is_empty() {
            return;
        }
        if self.0.len() == STDERR_TAIL {
            self.0.pop_front();
        }
        self.0.push_back(line.into_owned());
    }

    /// `headline`, then the tail on the lines below it.
    pub(crate) fn fatal(&self, headline: &str) -> String {
        let mut out = headline.to_string();
        for line in &self.0 {
            out.push('\n');
            out.push_str(line);
        }
        out
    }
}

/// Logs an event shape the Driver doesn't map, once per process; true the first time.
pub(crate) fn log_unknown(provider: &str, what: &str) -> bool {
    static SEEN: LazyLock<Mutex<HashSet<(String, String)>>> = LazyLock::new(Mutex::default);
    let first = SEEN.guard().insert((provider.to_string(), what.to_string()));
    if first {
        eprintln!("{provider}: unhandled {what}");
    }
    first
}

#[cfg(test)]
#[path = "../tests/unit/spawn.rs"]
mod tests;
