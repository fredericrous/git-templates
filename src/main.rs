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

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

mod dispatch;
mod git;
mod hooks;
mod pushrefs;
mod registry;
mod ui;
mod vocabulary;

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

    let push = pushrefs::PushRefs::default();
    let ctx = registry::Ctx {
        name: &hook,
        args: &rest,
        hooks_dir: &hooks_dir,
        push: &push,
    };
    let code = match registry::lookup(&hook) {
        Some(f) => f(&ctx),
        None => {
            eprintln!("githooks: unknown hook {hook:?}");
            2
        }
    };
    std::process::exit(code);
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
    // `parent()` is LEXICAL; `join("..")` is not. The latter makes the kernel
    // resolve `hooks/..`, which fails outright when the hooks directory does
    // not exist — and `git init --template=` creates no hooks directory. The
    // check then reports "no cherry-pick" for a reason that has nothing to do
    // with cherry-picks.
    hooks_dir
        .parent()
        .map(|d| d.join("CHERRY_PICK_HEAD").exists())
        .unwrap_or(false)
}
