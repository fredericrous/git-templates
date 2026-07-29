//! The two dispatchers.
//!
//! They are NOT the same shape, and both shapes are load-bearing:
//!
//! - `pre-commit` runs sub-hooks in PARALLEL and reports EVERY failure. Serial
//!   would be a visible slowdown on each commit; stopping at the first failure
//!   would hide the rest, so you'd fix one lint error, commit, and immediately
//!   meet the next.
//! - `pre-push` runs them SERIALLY and stops at the FIRST failure, naming just
//!   that hook. The steps are ordered and expensive (branch name, then rebase,
//!   then the whole test suite) and there is no point running tests after a
//!   rebase conflict.
//!
//! Resist the tempting shared `run_all` helper — collapsing these is the
//! obvious way to silently lose the distinction. `tests/pre-commit.test.zsh`
//! and `tests/pre-push.test.zsh` pin both.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{apply_skips, cherry_pick_in_progress, configured_skips, sub_hooks};

fn selected(hooks_dir: &Path, hook: &str) -> Vec<PathBuf> {
    apply_skips(sub_hooks(hooks_dir, hook), &configured_skips())
}

/// stdin is INHERITED, never read here. git feeds pre-push the pushed refs on
/// stdin and `pre-push-run-tests-js` consumes them; reading it in the
/// dispatcher would swallow the data before the sub-hook saw it.
fn spawn(path: &PathBuf, args: &[OsString]) -> std::io::Result<std::process::Child> {
    Command::new(path)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
}

/// Exit code of a child, mapping "killed by signal" to a non-zero code so a
/// segfaulting hook can never read as success.
fn wait_code(mut child: std::process::Child) -> i32 {
    match child.wait() {
        Ok(status) => status.code().unwrap_or(128),
        Err(_) => 1,
    }
}

pub fn pre_commit(hooks_dir: &Path, args: &[OsString]) -> i32 {
    if cherry_pick_in_progress(hooks_dir) {
        return 0;
    }
    let hooks = selected(hooks_dir, "pre-commit");
    if hooks.is_empty() {
        return 0;
    }

    // Spawn every sub-hook, THEN wait: the parallelism is the point.
    let mut running = Vec::new();
    for path in &hooks {
        match spawn(path, args) {
            Ok(child) => running.push((path.clone(), child)),
            Err(e) => {
                eprintln!("githooks: cannot run {}: {e}", path.display());
                running.clear();
                return 1;
            }
        }
    }

    let mut failed: Vec<PathBuf> = Vec::new();
    let mut exit_code = 0;
    for (path, child) in running {
        let code = wait_code(child);
        if code != 0 {
            // Last failure wins the exit code, as the zsh version did.
            exit_code = code;
            failed.push(path);
        }
    }

    // Every failure is listed — that is the whole reason this one is parallel.
    if exit_code != 0 {
        println!("\n🚨  Error raised by:");
        for f in &failed {
            println!("    - \u{1b}[38;5;208m{}\u{1b}[0m", f.display());
        }
    }
    exit_code
}

pub fn pre_push(hooks_dir: &Path, args: &[OsString]) -> i32 {
    // NB: no CHERRY_PICK_HEAD check here — the zsh pre-push had none either.
    for path in selected(hooks_dir, "pre-push") {
        let code = match spawn(&path, args) {
            Ok(child) => wait_code(child),
            Err(e) => {
                eprintln!("githooks: cannot run {}: {e}", path.display());
                1
            }
        };
        if code != 0 {
            // Singular, and stop here: the later steps are expensive and their
            // preconditions no longer hold.
            println!(
                "\n🚨  Error raised by hook \u{1b}[38;5;208m{}\u{1b}[0m",
                path.display()
            );
            return code;
        }
    }
    0
}
