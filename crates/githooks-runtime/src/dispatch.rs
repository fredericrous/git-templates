//! The two dispatchers.
//!
//! They are NOT the same shape, and both shapes are load-bearing:
//!
//! - `pre-commit` runs its checks CONCURRENTLY and reports EVERY failure.
//!   Serial would be a visible slowdown on each commit; stopping at the first
//!   failure would hide the rest, so you'd fix one lint error, commit, and
//!   immediately meet the next.
//! - `pre-push` runs them SERIALLY and stops at the FIRST failure, naming just
//!   that check. The steps are ordered and expensive (protected branch, then
//!   branch name, then rebase, then the whole test suite) and there is no point
//!   running tests after a rebase conflict.
//!
//! Resist the tempting shared `run_all` helper — collapsing these is the
//! obvious way to silently lose the distinction. `tests/dispatchers.rs` pins
//! both.
//!
//! Checks are FUNCTIONS in this binary, called directly. They used to be files:
//! `.git/hooks/pre-commit-*`, each an identical `sh` shim whose only job was to
//! re-exec this same binary and tell it its own name. One commit therefore cost
//! 27 processes — a shim, the binary, then 13 more shims and 13 more binaries —
//! to do work the binary already had in a table.
//!
//! Deleting that removed the filename glob (order was lexicographic, so a
//! rename could silently reorder a gate), the shebang emulation Windows needed
//! because it cannot execute a `#!` script, and the spawn plumbing under both.
//! Order is now a declared list in `registry`.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;

use crate::registry::{Ctx, HookFn, PRE_COMMIT_CHECKS, PRE_PUSH_CHECKS, REGISTRY};
use crate::ui::{highlight, warning_sign};
use crate::{cherry_pick_in_progress, configured_skips};

fn handler(name: &str) -> HookFn {
    REGISTRY
        .iter()
        .find(|(r, _)| *r == name)
        .expect("a declared check is registered — enforced by a registry test")
        .1
}

/// The declared checks, minus anything `hook.skip` filters out. `hook.skip`
/// yields substrings and matches by CONTAINS, exactly as it did against paths,
/// so existing config keeps working: `git config hook.skip ruff` still skips
/// `pre-commit-ruff`.
fn selected(list: &[&'static str]) -> Vec<&'static str> {
    let skips = configured_skips();
    let (kept, dropped): (Vec<_>, Vec<_>) = list
        .iter()
        .copied()
        .partition(|n| !skips.iter().any(|s| crate::skip_suppresses(n, s)));
    announce_skips(&dropped);
    kept
}

/// Say out loud which checks did not run.
///
/// A skip is otherwise invisible at exactly the moment it matters. With
/// `hook.skip = merge-conflict` set, a commit printed six green ticks and no
/// hint that a seventh check had been disabled — the developer sees a clean run
/// and concludes they are covered.
///
/// It is worse than it sounds, because `hook.skip` matches by SUBSTRING.
/// `hook.skip = e` suppresses all twenty checks, and `t` suppresses nineteen;
/// both are plausible shorthand rather than adversarial input. Without this
/// line, a commit under either looks indistinguishable from a commit that had
/// nothing to report.
///
/// One line, only when something was actually skipped, so a normal commit is
/// unchanged. This reaches every skip however it was created — hand-edited
/// config included — which no dashboard can claim.
fn announce_skips(dropped: &[&str]) {
    if dropped.is_empty() {
        return;
    }
    let plural = if dropped.len() == 1 {
        "check"
    } else {
        "checks"
    };
    println!(
        "{} {} {plural} skipped by {}: {}",
        warning_sign(),
        dropped.len(),
        highlight("hook.skip"),
        dropped.join(", ")
    );
}

/// Run every item concurrently and collect `(name, code)` in the INPUT order.
///
/// Extracted so the concurrency itself can be tested with a rendezvous instead
/// of a stopwatch — an earlier wall-clock test was flaky the moment the machine
/// was busy, and a threshold that trips under load teaches you to ignore it.
fn run_concurrently<F>(names: &[&'static str], run: F) -> Vec<(&'static str, i32)>
where
    F: Fn(&'static str) -> i32 + Sync,
{
    let slots: Vec<Mutex<Option<i32>>> = names.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for (name, slot) in names.iter().zip(&slots) {
            let run = &run;
            scope.spawn(move || {
                let code = run(name);
                *slot.lock().expect("poisoned") = Some(code);
            });
        }
    });
    names
        .iter()
        .zip(slots)
        .map(|(n, s)| (*n, s.into_inner().expect("poisoned").unwrap_or(1)))
        .collect()
}

pub fn pre_commit(ctx: &Ctx) -> i32 {
    if cherry_pick_in_progress(ctx.hooks_dir) {
        return 0;
    }
    let checks = selected(PRE_COMMIT_CHECKS);
    if checks.is_empty() {
        return 0;
    }

    let last_failure = AtomicI32::new(0);
    let results = run_concurrently(&checks, |name| {
        let sub = Ctx {
            name,
            args: ctx.args,
            hooks_dir: ctx.hooks_dir,
            push: ctx.push,
        };
        let code = handler(name)(&sub);
        if code != 0 {
            // Last failure wins the exit code, as the zsh version did.
            last_failure.store(code, Ordering::Relaxed);
        }
        code
    });

    let failed: Vec<&str> = results
        .iter()
        .filter(|(_, c)| *c != 0)
        .map(|(n, _)| *n)
        .collect();
    if failed.is_empty() {
        return 0;
    }
    // Every failure is listed — that is the whole reason this one is concurrent.
    println!("\n🚨  Error raised by:");
    for f in &failed {
        println!("    - \u{1b}[38;5;208m{f}\u{1b}[0m");
    }
    last_failure.load(Ordering::Relaxed)
}

pub fn pre_push(ctx: &Ctx) -> i32 {
    // NB: no CHERRY_PICK_HEAD check here — the zsh pre-push had none either.
    for name in selected(PRE_PUSH_CHECKS) {
        let sub = Ctx {
            name,
            args: ctx.args,
            hooks_dir: ctx.hooks_dir,
            push: ctx.push,
        };
        let code = handler(name)(&sub);
        if code != 0 {
            // Singular, and stop here: the later steps are expensive and their
            // preconditions no longer hold.
            println!("\n🚨  Error raised by hook \u{1b}[38;5;208m{name}\u{1b}[0m");
            return code;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    /// Concurrency proved by RENDEZVOUS, not by a stopwatch: every task must
    /// observe all the others arrive. Were the runner serial, the first task
    /// would wait alone, time out, and return non-zero — a failure, not a hang.
    #[test]
    fn run_concurrently_actually_overlaps() {
        static ARRIVED: AtomicUsize = AtomicUsize::new(0);
        ARRIVED.store(0, Ordering::SeqCst);
        let names: Vec<&'static str> = vec!["a", "b", "c", "d"];
        let n = names.len();

        let out = run_concurrently(&names, move |_| {
            ARRIVED.fetch_add(1, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(10);
            while ARRIVED.load(Ordering::SeqCst) < n {
                if Instant::now() > deadline {
                    return 1; // never met the others — execution was serial
                }
                std::thread::yield_now();
            }
            0
        });
        assert!(
            out.iter().all(|(_, c)| *c == 0),
            "tasks did not overlap: {out:?}"
        );
    }

    #[test]
    fn results_come_back_in_input_order() {
        let names: Vec<&'static str> = vec!["first", "second", "third"];
        let out = run_concurrently(&names, |n| if n == "second" { 7 } else { 0 });
        assert_eq!(out, vec![("first", 0), ("second", 7), ("third", 0)]);
    }

    /// `hook.skip` matches by substring, as it did when it filtered paths.
    #[test]
    fn skips_match_by_substring() {
        let all = ["pre-commit-ruff", "pre-commit-prettier"];
        let skips = ["ruff".to_string()];
        let kept: Vec<_> = all
            .iter()
            .copied()
            .filter(|n| !skips.iter().any(|s| n.contains(s.as_str())))
            .collect();
        assert_eq!(kept, vec!["pre-commit-prettier"]);
    }
}
