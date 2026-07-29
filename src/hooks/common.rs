//! Shared plumbing for the linter-orchestration hooks.
//!
//! Nine of them do the same four things: collect staged files of some kind,
//! bail out if there are none, resolve a tool, run it. In shell that was ~65
//! lines apiece, mostly duplicated; here it is a handful of helpers and each
//! hook keeps only what is actually specific to it.

use crate::git;
use crate::ui::{ERROR_SIGN, VALID_SIGN, WARNING_SIGN};
use std::path::Path;
use std::process::{Command, Stdio};

/// Staged files, deletions excluded, whose name ends with one of `exts`.
/// An empty `exts` returns them all.
pub fn staged_files(exts: &[&str]) -> Vec<String> {
    let Some(out) = git::stdout(&["diff", "--diff-filter=d", "--cached", "--name-only"]) else {
        return Vec::new();
    };
    out.lines()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .filter(|f| exts.is_empty() || exts.iter().any(|e| f.ends_with(e)))
        .map(str::to_owned)
        .collect()
}

/// Repo root, or "." when git cannot say.
pub fn repo_root() -> String {
    git::stdout(&["rev-parse", "--show-toplevel"]).unwrap_or_else(|| ".".into())
}

/// Resolve a tool, preferring the repo's PINNED copy so the hook matches CI.
///
///
/// Order: `<root>/node_modules/.bin/<tool>`, then the MAIN worktree's (a linked
/// worktree has no node_modules of its own — this is why the shell version
/// consulted the git common dir), then PATH.
pub fn resolve_tool(root: &str, tool: &str) -> Option<Vec<String>> {
    let local = format!("{root}/node_modules/.bin/{tool}");
    if Path::new(&local).is_file() {
        return Some(vec![local]);
    }
    if let Some(common) = git::stdout(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
    {
        if let Some(main) = Path::new(&common).parent() {
            let p = main.join("node_modules/.bin").join(tool);
            if p.is_file() {
                return Some(vec![p.to_string_lossy().into_owned()]);
            }
        }
    }
    if which(tool).is_some() {
        return Some(vec![tool.to_string()]);
    }
    // `npx --no-install`: never silently download a random latest version — a
    // hook that quietly pulls a different linter than CI uses is worse than one
    // that skips.
    if which("npx").is_some()
        && Command::new("npx")
            .args(["--no-install", tool, "--version"])
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    {
        return Some(vec![
            "npx".to_string(),
            "--no-install".to_string(),
            tool.to_string(),
        ]);
    }
    None
}

/// First match for `tool` on PATH.
pub fn which(tool: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(tool);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

/// The first of `names` that exists at the repo root — how these hooks decide
/// a repo has opted into a tool.
pub fn first_existing(root: &str, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find(|n| Path::new(root).join(n).exists())
        .map(|n| (*n).to_string())
}

/// Run `argv` from `root`, inheriting stdio. True when it exits 0.
pub fn run(root: &str, argv: &[String], extra: &[String]) -> bool {
    let Some((program, rest)) = argv.split_first() else {
        return true;
    };
    Command::new(program)
        .args(rest)
        .args(extra)
        .current_dir(root)
        .stdin(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn ok(msg: &str) {
    println!("{VALID_SIGN} {msg}");
}
pub fn fail(msg: &str) {
    println!("{ERROR_SIGN} {msg}");
}
pub fn warn(msg: &str) {
    println!("{WARNING_SIGN} {msg}");
}

/// Orange, for the fragments these hooks highlight.
pub fn hl(s: &str) -> String {
    crate::ui::color(s, "208")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_finds_a_real_binary_and_not_a_fake_one() {
        assert!(which("git").is_some());
        assert!(which("definitely-not-a-real-binary-xyz").is_none());
    }

    #[test]
    fn first_existing_picks_the_earliest_present_name() {
        let dir = std::env::temp_dir().join("githooks-first-existing-test");
        let _ = std::fs::create_dir_all(&dir);
        let root = dir.to_string_lossy().into_owned();
        let _ = std::fs::write(dir.join("second"), "x");
        assert_eq!(
            first_existing(&root, &["first", "second", "third"]).as_deref(),
            Some("second")
        );
        assert_eq!(first_existing(&root, &["nope"]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
