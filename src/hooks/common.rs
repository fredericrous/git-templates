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
    // Same extension problem as `which`: an npm-installed binary is `eslint.cmd`
    // on Windows, so the bare name misses the repo's PINNED copy and the hook
    // silently falls through to an ambient one.
    if let Some(p) = in_bin_dir(&format!("{root}/node_modules/.bin"), tool) {
        return Some(vec![p]);
    }
    if let Some(common) = git::stdout(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
    {
        if let Some(main) = Path::new(&common).parent() {
            if let Some(p) = in_bin_dir(&main.join("node_modules/.bin").to_string_lossy(), tool) {
                return Some(vec![p]);
            }
        }
    }
    if let Some(full) = which(tool) {
        return Some(vec![full]);
    }
    // `npx --no-install`: never silently download a random latest version — a
    // hook that quietly pulls a different linter than CI uses is worse than one
    // that skips.
    if which("npx").is_some()
        && Command::new(program("npx"))
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
            program("npx"),
            "--no-install".to_string(),
            tool.to_string(),
        ]);
    }
    None
}

/// First match for `tool` on PATH.
///
/// Windows executables carry an extension — `git` is `git.exe`, an npm-installed
/// `eslint` is `eslint.cmd` — so the bare name finds nothing there. PATHEXT is
/// the OS's own list of what counts as executable; fall back to the usual set
/// when it is unset. Found by the Windows CI job on its first run, where
/// `which("git")` returned None on a machine that plainly has git.
pub fn which(tool: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_lowercase())
            .collect()
    } else {
        Vec::new()
    };
    for dir in std::env::split_paths(&path) {
        // On Windows the EXTENSION forms come first. A node install ships both
        // `npm` (an extensionless shell script, for MSYS) and `npm.cmd` in the
        // same directory; preferring the bare name hands CreateProcess a shell
        // script it cannot execute — "%1 is not a valid Win32 application" —
        // and the hook reports an installed tool as broken.
        for e in &exts {
            let c = dir.join(format!("{tool}{e}"));
            if c.is_file() {
                return Some(c.to_string_lossy().into_owned());
            }
        }
        let bare = dir.join(tool);
        if bare.is_file() {
            return Some(bare.to_string_lossy().into_owned());
        }
    }
    None
}

/// `<dir>/<tool>`, trying the Windows executable extensions too.
fn in_bin_dir(dir: &str, tool: &str) -> Option<String> {
    let bare = Path::new(dir).join(tool);
    if bare.is_file() {
        return Some(bare.to_string_lossy().into_owned());
    }
    if cfg!(windows) {
        for e in [".cmd", ".exe", ".bat", ".ps1"] {
            let c = Path::new(dir).join(format!("{tool}{e}"));
            if c.is_file() {
                return Some(c.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// Resolve a tool name to a full path for spawning.
///
/// `Command::new("npm")` cannot execute `npm.cmd`: Rust does no PATHEXT
/// resolution, so on Windows every bare-name spawn fails with "program not
/// found" and the hook reports the tool as broken rather than absent. Found by
/// the Windows job on its first FULL-suite run — the smoke never spawned a
/// tool, so it could not have surfaced this.
///
/// Falls back to the name unchanged, so a caller still gets a sensible error.
pub fn program(name: &str) -> String {
    which(name).unwrap_or_else(|| name.to_string())
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

    /// On Windows a tool can exist BOTH as an extensionless shell script and as
    /// a .cmd/.exe in the same directory; only the latter is executable by
    /// CreateProcess, so the extension forms must win.
    #[test]
    #[cfg(windows)]
    fn windows_prefers_an_executable_extension_over_a_bare_file() {
        let dir = std::env::temp_dir().join("githooks-which-order");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("faketool"), "#!/bin/sh\n").unwrap();
        std::fs::write(dir.join("faketool.cmd"), "@echo off\n").unwrap();
        let saved = std::env::var_os("PATH");
        std::env::set_var("PATH", &dir);
        let found = which("faketool").unwrap();
        if let Some(p) = saved {
            std::env::set_var("PATH", p);
        }
        assert!(found.ends_with(".cmd"), "got {found}");
        let _ = std::fs::remove_dir_all(&dir);
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
