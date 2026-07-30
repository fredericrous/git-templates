//! The git-templates hook logic: registry, dispatchers and every check.
//!
//! This is a library so that more than one binary can hold the same truth about
//! what a hook IS. `githooks` (the commit path) executes the checks;
//! `githooks-fleet` reports on how they are installed across the fleet. Before
//! the split there was no lib target at all, which is why `cargo test --lib`
//! failed outright.
//!
//! **This crate must never gain an external dependency.** The hook binary
//! depends on it, so anything added here reaches every commit transitively —
//! and the entire Rust migration existed to remove exactly that kind of
//! requirement. ratatui and friends belong in `githooks-fleet`.
//!
//! Hooks are invoked through a thin `sh` shim at each hook path, which passes
//! the hooks directory it lives in:
//!
//! ```text
//! githooks --hooks-dir <dir> pre-commit [args…]
//! ```

pub mod check;
pub mod dispatch;
pub mod git;
pub mod hooks;
pub mod pushrefs;
pub mod registry;
pub mod ui;
pub mod vocabulary;

use std::path::Path;
use std::process::{Command, Stdio};

/// `git config --get-all hook.skip`, or empty when unset/unavailable.
/// Does `skip`, as configured in `hook.skip`, suppress `check`?
///
/// SUBSTRING, not equality — `hook.skip = clippy` disables
/// `pre-commit-clippy`, which is what makes the config usable by hand. It is
/// also why `hook.skip = e` disables everything: every check name contains an
/// `e`. That is a sharp edge, not a bug, and the dispatcher announces the
/// consequence on every commit.
///
/// Defined ONCE because three callers need it and a fourth is coming: the
/// dispatcher decides what runs, the fleet view reports where a check applies,
/// and the skip resolver computes blast radius. A reimplementation that
/// disagreed would have the dashboard claim a check is active while the
/// dispatcher skips it — a difference nobody would notice until it mattered.
pub fn skip_suppresses(check: &str, skip: &str) -> bool {
    check.contains(skip)
}

/// Print every check: stage, scope, and whether it would fire in this
/// repository.
///
/// "Why didn't prettier run?" was previously a code-reading exercise. The
/// answer is a check's `Scope` evaluated against the repo's tracked files, so
/// the tool can simply say it.
pub fn list_checks() {
    use crate::check::Stage;
    let paths: Vec<String> = Command::new("git")
        .args(["ls-files"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let skips = configured_skips();

    for stage in [Stage::PreCommit, Stage::PrePush] {
        println!("{}", ui::highlight(stage.as_str()));
        for c in registry::stage_checks(stage) {
            let skipped = skips.iter().any(|s| skip_suppresses(c.name, s));
            let applies = c.scope.matches(&paths);
            // Three states, three glyphs: a check that is correctly silent must
            // never look like one that is broken or disabled.
            let (glyph, why) = if skipped {
                ('⊘', "skipped via hook.skip".to_string())
            } else if applies {
                ('●', String::new())
            } else {
                ('○', format!("inert here — needs {}", describe(c.scope)))
            };
            println!("  {glyph} {:<28} {why}", c.name);
        }
    }
    println!();
    println!("  ● runs here   ○ inert   ⊘ skipped via hook.skip");
}

fn describe(s: crate::check::Scope) -> String {
    let files = if s.files.is_empty() {
        String::new()
    } else {
        s.files.join(" ")
    };
    let opt = s.opt_in.join(" | ");
    match (files.is_empty(), opt.is_empty()) {
        (false, false) => format!("{files} + {opt}"),
        (false, true) => files,
        (true, false) => opt,
        (true, true) => "nothing".into(),
    }
}

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
