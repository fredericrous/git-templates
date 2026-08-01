//! Thin wrappers over the `git` calls the hooks make.

use std::process::{Command, Stdio};

/// stdout of a git command, trimmed. `None` when git itself failed — which the
/// hooks treat as "cannot tell, do not block", never as "empty".
pub fn stdout(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The same, run inside `dir`.
///
/// The dashboard asks about repositories it is not standing in, and must get
/// the answer git would give THERE — config is per-repository, so asking from
/// the wrong directory returns the wrong severity.
pub fn stdout_in(dir: &std::path::Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// stdout of a git command that itself reads a list from stdin — `diff-tree
/// --stdin`, fed a list of commits, is the only caller today. Lossy but
/// untrimmed: every line is a path, and the caller trims those itself.
pub fn stdout_piped(args: &[&str], stdin: &str) -> Option<String> {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(stdin.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Raw stdout, untrimmed and not lossy — for a patch, where a trailing newline
/// and any byte in a binary hunk are load-bearing.
pub fn stdout_raw(args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// True when the command exits 0. Output discarded.
pub fn succeeds(args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
