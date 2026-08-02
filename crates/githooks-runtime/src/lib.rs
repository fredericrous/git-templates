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
pub mod install;
pub mod json;
pub mod manifest;
pub mod pushed_tree;
pub mod pushrefs;
pub mod registry;
pub mod staged_only;
pub mod trust;
pub mod ui;
pub mod vocabulary;

use std::path::Path;
use std::process::{Command, Stdio};

/// `git config --get-all hook.skip`, or empty when unset/unavailable.
/// The two triggers a check can be attached to, as they are spelled in config.
///
/// Deliberately the same strings as `Stage::as_str`, and
/// `every_id_agrees_with_its_declared_stage` keeps them that way.
pub const TRIGGERS: [&str; 2] = ["pre-commit", "pre-push"];

/// How specifically a configured value names a check.
///
/// Ordered, so that when several keys match one check the most specific wins —
/// which only matters for `githooks.severity`, since a skip is a boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Match {
    /// `pre-commit` — every check on that trigger.
    Trigger,
    /// `clippy` — that check, whichever trigger it is on.
    ShortName,
    /// `pre-commit-clippy` — this check and no other.
    FullId,
}

/// A check's id without its trigger — `pre-commit-clippy` → `clippy`.
///
/// For DISPLAY only, wherever the trigger is already established by a heading
/// or a neighbouring column. Under a `pre-commit` heading, printing
/// `pre-commit-clippy` on every row spends eleven columns restating what the
/// heading said. Never write this to config or compare against it: two checks
/// can share a short name, and telling them apart is what the id is for.
pub fn short_name(check: &str) -> &str {
    for trigger in TRIGGERS {
        if let Some(short) = check
            .strip_prefix(trigger)
            .and_then(|rest| rest.strip_prefix('-'))
        {
            return short;
        }
    }
    check
}

/// Does `pattern`, as written in `hook.skip` or `githooks.severity.<pattern>`,
/// name `check`?
///
/// A check's id is `<trigger>-<name>`, and exactly three things name it:
///
/// | written | means |
/// |---|---|
/// | `pre-commit-clippy` | that one check |
/// | `pre-commit`        | every check on that trigger |
/// | `clippy`            | that check, on any trigger |
///
/// Three exact comparisons. **No substring.** The previous rule was
/// `check.contains(skip)`, which made `hook.skip = clippy` work by accident of
/// reach — and `hook.skip = e` disable all twenty checks by the same accident,
/// and `lint-js` silently also suppress `lint-json-yaml`. Naming the three
/// things a user actually means keeps every useful case and removes every
/// sharp edge, including the one the old doc comment called "not a bug".
///
/// This reads the trigger out of the ID, which is not the same as deriving a
/// check's stage: `Stage` remains a declared field and is what the dispatcher
/// obeys. Here we are parsing an identifier a human typed.
///
/// Defined ONCE because four callers need it — the dispatcher decides what
/// runs, the severity resolver decides what blocks, the fleet view reports
/// where a check applies, and the skip resolver computes reach. A
/// reimplementation that disagreed would have the dashboard claim a check is
/// active while the dispatcher skips it.
pub fn names_check(check: &str, pattern: &str) -> Option<Match> {
    if check == pattern {
        return Some(Match::FullId);
    }
    for trigger in TRIGGERS {
        let Some(short) = check
            .strip_prefix(trigger)
            .and_then(|rest| rest.strip_prefix('-'))
        else {
            continue;
        };
        // An id carries one trigger, so the first that matches is the answer.
        if pattern == trigger {
            return Some(Match::Trigger);
        }
        if pattern == short {
            return Some(Match::ShortName);
        }
        return None;
    }
    None
}

/// Does `skip`, as configured in `hook.skip`, suppress `check`?
pub fn skip_suppresses(check: &str, skip: &str) -> bool {
    names_check(check, skip).is_some()
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

    let broken: std::collections::BTreeMap<&str, &str> = manifest::externals()
        .iter()
        .filter_map(|external| match &external.kind {
            manifest::Kind::Unusable { why } => Some((external.id.as_str(), why.as_str())),
            manifest::Kind::Runnable { .. } => None,
        })
        .collect();

    for stage in [Stage::PreCommit, Stage::PrePush] {
        println!("{}", ui::highlight(stage.as_str()));
        // Externals are listed here too, and marked, because the question this
        // command answers — "would this run here?" — is asked most often about
        // the check somebody just added to `.githooks.conf`.
        for check in registry::all_stage_checks(stage) {
            let name = check.name();
            let skipped = skips.iter().any(|s| skip_suppresses(name, s));
            let applies = check.scope().matches(&paths);
            // Four states, four glyphs: a check that is correctly silent must
            // never look like one that is disabled, and neither must look like
            // one whose declaration could not be read.
            let (glyph, why) = if let Some(w) = broken.get(name) {
                ('✗', format!("{} {w}", manifest::MANIFEST))
            } else if skipped {
                ('⊘', "skipped via hook.skip".to_string())
            } else if applies {
                ('●', String::new())
            } else {
                (
                    '○',
                    format!("inert here — needs {}", describe(check.scope())),
                )
            };
            // The SHORT name: this loop is already inside a `pre-commit` /
            // `pre-push` heading, so printing the trigger on all twenty rows
            // restates the heading twenty times and pushes the reason — the
            // part that differs per row — eleven columns to the right.
            //
            // Where a check CAME FROM belongs next to its name, not appended
            // after a reason that is often empty. A reader scanning this list
            // wants to know which of these their repository added.
            let label = if is_external(name) {
                format!("{} (declared)", short_name(name))
            } else {
                short_name(name).to_string()
            };
            println!("  {glyph} {label:<26} {why}");
        }
    }
    println!();
    println!("  ● runs here   ○ inert   ⊘ skipped via hook.skip   ✗ declaration unusable");
}

fn is_external(name: &str) -> bool {
    manifest::externals()
        .iter()
        .any(|external| external.id == name)
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
/// Which git operations are part-way through, from the markers in `$GIT_DIR`.
///
/// `hooks_dir.parent()` is the git dir. `parent()` is LEXICAL; `join("..")` is
/// not — the latter makes the kernel resolve `hooks/..`, which fails outright
/// when the hooks directory does not exist, and `git init --template=` creates
/// none. The check then reports "no operation in progress" for a reason that
/// has nothing to do with git operations. That was learned once, for
/// cherry-picks; it applies to all five.
pub fn git_states_in_progress(hooks_dir: &Path) -> Vec<crate::check::GitState> {
    let Some(git_dir) = hooks_dir.parent() else {
        return Vec::new();
    };
    crate::check::GitState::ALL
        .into_iter()
        .filter(|state| {
            state
                .markers()
                .iter()
                .any(|marker| git_dir.join(marker).exists())
        })
        .collect()
}

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

#[cfg(test)]
mod naming {
    use super::*;

    /// The three things a user can write, and what each reaches.
    #[test]
    fn three_ways_to_name_a_check() {
        assert_eq!(
            names_check("pre-commit-clippy", "pre-commit-clippy"),
            Some(Match::FullId)
        );
        assert_eq!(
            names_check("pre-commit-clippy", "pre-commit"),
            Some(Match::Trigger)
        );
        assert_eq!(
            names_check("pre-commit-clippy", "clippy"),
            Some(Match::ShortName)
        );
    }

    /// The hazards the old substring rule created, all gone by construction.
    #[test]
    fn nothing_matches_by_accident() {
        // `hook.skip = e` disabled all twenty checks. It now reaches nothing.
        for pattern in ["e", "t", "i", ""] {
            assert_eq!(
                names_check("pre-commit-clippy", pattern),
                None,
                "{pattern:?}"
            );
        }
        // A partial word is not a name.
        assert_eq!(names_check("pre-commit-clippy", "clip"), None);
        assert_eq!(names_check("pre-commit-clippy", "lint"), None);
        // The wrong trigger reaches nothing.
        assert_eq!(names_check("pre-commit-clippy", "pre-push"), None);
        // And the empty string names nothing, rather than everything — git
        // stores `hook.skip` with no value as exactly this.
        assert_eq!(names_check("pre-commit-clippy", ""), None);
    }

    /// The coupling `docs/hook-skip-management.md` warned about: `lint-js` is a
    /// substring of `lint-json-yaml`, so skipping one used to skip both.
    #[test]
    fn a_short_name_does_not_reach_a_longer_one() {
        assert!(names_check("pre-commit-lint-json-yaml", "lint-js").is_none());
        assert_eq!(
            names_check("pre-commit-lint-js", "lint-js"),
            Some(Match::ShortName)
        );
        assert_eq!(
            names_check("pre-commit-lint-json-yaml", "lint-json-yaml"),
            Some(Match::ShortName)
        );
    }

    /// The one value that exists in the real fleet.
    #[test]
    fn the_fleets_only_skip_still_resolves() {
        assert_eq!(
            names_check("pre-push-run-tests-js", "run-tests-js"),
            Some(Match::ShortName)
        );
    }

    /// A trigger reaches every check on it and none on the other.
    #[test]
    fn a_trigger_reaches_its_own_stage_only() {
        let pre_commit = registry::CHECKS
            .iter()
            .filter(|c| names_check(c.name, "pre-commit").is_some())
            .count();
        let pre_push = registry::CHECKS
            .iter()
            .filter(|c| names_check(c.name, "pre-push").is_some())
            .count();
        assert_eq!(pre_commit + pre_push, registry::CHECKS.len());
        assert!(pre_commit > 0 && pre_push > 0);
    }

    /// Specificity ordering, which decides severity when several keys apply.
    #[test]
    fn a_full_id_outranks_a_short_name_outranks_a_trigger() {
        assert!(Match::FullId > Match::ShortName);
        assert!(Match::ShortName > Match::Trigger);
    }

    /// The resolver reads the trigger out of the ID. That is only sound while
    /// every ID agrees with the stage its check actually declares — so it is
    /// checked rather than assumed.
    #[test]
    fn every_id_agrees_with_its_declared_stage() {
        for check in registry::CHECKS {
            assert_eq!(
                names_check(check.name, check.stage.as_str()),
                Some(Match::Trigger),
                "{} declares {:?} but its id says otherwise",
                check.name,
                check.stage
            );
        }
    }
}
