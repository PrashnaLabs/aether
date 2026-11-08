use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::{Output, QUICK, STDERR_CAP, binary, drain, git_error, stop};

const MOVED: &str = "This change moved; refresh and try again.";

/// What a hunk action does with one `@@` hunk of a file's diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkOp {
    /// Adds a work-tree hunk to the index.
    Stage,
    /// Takes a staged hunk back out of the index.
    Unstage,
    /// Drops a work-tree hunk from the file; the index is left alone.
    Revert,
}

/// Applies hunk `ix` of `diff`, one file's text diff from [`crate::diff`]:
/// its work-tree diff for `Stage` and `Revert`, its staged one for `Unstage`.
/// `top` is the work tree's top folder. Blocking.
pub fn apply_hunk(top: &Path, diff: &str, ix: usize, op: HunkOp) -> Result<(), String> {
    let patch = hunk_patch(diff, ix).ok_or(MOVED)?;
    // lossy decoding put U+FFFD where the file has other bytes; staging it would write that
    if patch.contains('\u{FFFD}') {
        return Err("This file isn't UTF-8 text, so its hunks can't be applied one by one.".into());
    }
    // a/ b/ are what `git apply -p1` strips; diff.noprefix could aim a hunk at another file
    let mut header = patch.lines().take_while(|line| !line.starts_with("@@"));
    if !header.any(|line| line.starts_with("+++ b/") || line.starts_with("+++ \"b/")) {
        return Err("Hunk actions need git's default a/ and b/ diff prefixes; \
                    check diff.noprefix and diff.mnemonicPrefix."
            .into());
    }
    let mut args = vec!["apply", "--recount", "--whitespace=nowarn"];
    args.extend(match op {
        HunkOp::Stage => &["--cached"][..],
        HunkOp::Unstage => &["--cached", "--reverse"],
        HunkOp::Revert => &["--reverse"],
    });
    args.push("-");
    let out = run_stdin(top, &args, patch)?;
    if out.ok {
        Ok(())
    } else if out.stderr.contains("patch does not apply") {
        Err(MOVED.into())
    } else {
        Err(git_error(&out))
    }
}

/// Hunk `ix` as a patch of its own: its file's headers, as git wrote them
/// but for mode lines, and that one `@@` hunk. Line endings are kept byte for byte.
fn hunk_patch(diff: &str, ix: usize) -> Option<String> {
    let (mut header, mut in_header, mut seen) = (String::new(), false, 0);
    let mut patch: Option<String> = None;
    for line in diff.split_inclusive('\n') {
        let file = line.starts_with("diff --git ");
        let hunk = line.starts_with("@@");
        // a mode change would ride along with every hunk
        let mode = line.starts_with("old mode ") || line.starts_with("new mode ");
        if patch.is_some() && (file || hunk) {
            break;
        }
        if file {
            header.clear();
            in_header = true;
        }
        if hunk {
            in_header = false;
            if seen == ix {
                patch = Some(header.clone());
            }
            seen += 1;
        }
        match &mut patch {
            Some(patch) => patch.push_str(line),
            None if in_header && !mode => header.push_str(line),
            None => {}
        }
    }
    patch
}

/// Runs git in `top` with `input` on stdin, stopping it after [`QUICK`].
fn run_stdin(top: &Path, args: &[&str], input: String) -> Result<Output, String> {
    let git = binary()
        .ok_or("Git isn't available. Install git or the Xcode Command Line Tools, then retry.")?;
    let mut child = aether_core::command(git)
        .current_dir(top)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env("LC_ALL", "C")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't run git in {}: {e}", top.display()))?;
    let stderr = drain(child.stderr.take(), STDERR_CAP);
    if let Some(mut stdin) = child.stdin.take() {
        std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    }
    let deadline = Instant::now() + QUICK;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                stop(child);
                return Err(format!("Git took longer than {} s and was stopped.", QUICK.as_secs()));
            }
        }
    };
    let stderr = stderr.recv_timeout(Duration::from_secs(1)).unwrap_or_default();
    Ok(Output {
        ok: status.success(),
        stdout: String::new(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

#[cfg(test)]
#[path = "../tests/unit/hunk.rs"]
mod tests;
