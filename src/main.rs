//! The git-templates hooks as one binary.
//!
//! Phase 0 (see docs/rust-migration.md): this implements the two DISPATCHERS
//! only — `pre-commit` and `pre-push`. They discover and run the existing
//! script sub-hooks exactly as the zsh versions did. No hook logic has moved
//! yet, so behaviour must be indistinguishable.
//!
//! Invoked through a thin `sh` shim at each hook path, which passes the hooks
//! directory it lives in:
//!
//!     githooks --hooks-dir <dir> pre-commit [args…]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod dispatch;
mod git;
mod hooks;
mod ui;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let mut hooks_dir: Option<PathBuf> = None;
    let mut hook: Option<String> = None;
    let mut rest: Vec<OsString> = Vec::new();

    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--hooks-dir") => {
                hooks_dir = args.next().map(PathBuf::from);
            }
            _ if hook.is_none() => {
                hook = a.to_str().map(str::to_owned);
            }
            _ => {
                rest.push(a);
                rest.extend(args.by_ref());
                break;
            }
        }
    }

    let (Some(hooks_dir), Some(hook)) = (hooks_dir, hook) else {
        eprintln!("usage: githooks --hooks-dir <dir> <hook-name> [args…]");
        std::process::exit(2);
    };

    // Sub-hook shims keep their original filenames, so the name arrives with a
    // `.zsh` / `.js` suffix that no longer describes anything. Strip it rather
    // than rename the files: the existing tests resolve hooks by those exact
    // paths (docs/rust-migration.md), and renaming is a post-migration tidy-up.
    let hook = hook
        .strip_suffix(".zsh")
        .or_else(|| hook.strip_suffix(".js"))
        .unwrap_or(&hook)
        .to_owned();

    let code = match hook.as_str() {
        "pre-commit" => dispatch::pre_commit(&hooks_dir, &rest),
        "pre-push" => dispatch::pre_push(&hooks_dir, &rest),
        "pre-push-branch-pattern" => hooks::branch_pattern::run(&rest),
        "pre-commit-usual-name" => hooks::usual_name::run(&rest),
        "prepare-commit-msg" => hooks::prepare_commit_msg::run(&rest),
        "pre-push-pull-rebase" => hooks::pull_rebase::run(&rest),
        "commit-msg" => hooks::commit_msg::run(&rest),
        "pre-commit-ban-terms" => hooks::ban_terms::run(&hook, &rest),
        "pre-push-run-tests-js" => hooks::run_tests::run(&rest),
        other => {
            eprintln!("githooks: unknown hook {other:?}");
            2
        }
    };
    std::process::exit(code);
}

/// Sub-hooks for `hook`, in the order the shell glob produced: `<hook>-*` in
/// `dir`, sorted. Order is not cosmetic — pre-push relies on branch-pattern
/// running before pull-rebase before the test suite.
pub fn sub_hooks(dir: &Path, hook: &str) -> Vec<PathBuf> {
    let prefix = format!("{hook}-");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    // BTreeSet gives the glob's lexicographic order, deterministically.
    let mut found = BTreeSet::new();
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(&prefix) {
            found.insert(e.path());
        }
    }
    found.into_iter().collect()
}

/// Drop sub-hooks the user opted out of. `git config --get-all hook.skip`
/// yields substrings; the zsh dispatcher removed any path CONTAINING one
/// (`${HOOKS_PATH:#*$i*}`), which is what makes
/// `git -c hook.skip=package-lock commit` work. Substring, not equality.
pub fn apply_skips(hooks: Vec<PathBuf>, skips: &[String]) -> Vec<PathBuf> {
    if skips.is_empty() {
        return hooks;
    }
    hooks
        .into_iter()
        .filter(|p| {
            let s = p.to_string_lossy();
            !skips
                .iter()
                .any(|skip| !skip.is_empty() && s.contains(skip.as_str()))
        })
        .collect()
}

/// `git config --get-all hook.skip`, or empty when unset/unavailable.
pub fn configured_skips() -> Vec<String> {
    let Ok(out) = Command::new("git")
        .args(["config", "--get-all", "hook.skip"])
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// True during a cherry-pick, where the zsh `pre-commit` exited 0 immediately.
/// The marker sits next to the hooks directory, i.e. in `.git/`.
pub fn cherry_pick_in_progress(hooks_dir: &Path) -> bool {
    hooks_dir.join("..").join("CHERRY_PICK_HEAD").exists()
}
