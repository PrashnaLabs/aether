use std::process::{Command, Stdio};

/// `Command::new` that never flashes a console window when a GUI app spawns it on Windows.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let cmd = Command::new(program);
    #[cfg(windows)]
    let cmd = {
        use std::os::windows::process::CommandExt;
        let mut cmd = cmd;
        cmd.creation_flags(0x0800_0000);
        cmd
    };
    cmd
}

/// Signals `pid` (or its whole group/tree). `sig` is a `kill` name like "TERM", "KILL" or "0"
/// (liveness probe). Windows has no signals: anything but "0" is a forced taskkill, which
/// also ends the tree when `tree` is set.
pub fn signal_process(pid: u32, sig: &str, tree: bool) -> bool {
    let pid = pid.to_string();
    #[cfg(unix)]
    let mut cmd = {
        let mut cmd = command("/bin/kill");
        cmd.args([format!("-{sig}"), "--".into(), if tree { format!("-{pid}") } else { pid }]);
        cmd
    };
    #[cfg(windows)]
    let mut cmd = {
        if sig == "0" {
            let out = command("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
                .output();
            return out
                .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")));
        }
        let mut cmd = command("taskkill");
        cmd.args(["/PID", &pid, "/F"]);
        if tree {
            cmd.arg("/T");
        }
        cmd
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// `N` bytes from the OS CSPRNG. Panics if the OS has none; every caller is minting a secret or id.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("OS random source");
    bytes
}
