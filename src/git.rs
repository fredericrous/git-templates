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
