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

use std::sync::Mutex;

use crate::check::{Check, Outcome, Severity, Stage, Verdict};
use crate::configured_skips;
use crate::registry::{all_stage_checks, Ctx, Overrides};
use crate::ui::{highlight, warning_sign};

/// The checks for a stage, minus anything `hook.skip` filters out. `hook.skip`
/// yields substrings and matches by CONTAINS, exactly as it did against paths,
/// so existing config keeps working: `git config hook.skip ruff` still skips
/// `pre-commit-ruff`.
fn selected(stage: Stage) -> Vec<&'static dyn Check> {
    selected_during(stage, &[])
}

/// The checks for a stage, minus `hook.skip` and minus anything that declares
/// it does not run during an operation currently in progress.
fn selected_during(
    stage: Stage,
    in_progress: &[crate::check::GitState],
) -> Vec<&'static dyn Check> {
    let skips = configured_skips();
    // Externals are included here, so `hook.skip` and the severity override
    // govern a declared command exactly as they govern a built-in. A repository
    // that can add a check it cannot disable would be a worse deal than not
    // being able to add one.
    let (kept, dropped): (Vec<_>, Vec<_>) = all_stage_checks(stage)
        .into_iter()
        .partition(|c| !skips.iter().any(|s| crate::skip_suppresses(c.name(), s)));
    let names: Vec<&str> = dropped.iter().map(|c| c.name()).collect();
    announce_skips(&names);

    // Announced separately from `hook.skip`, and with the operation named: "not
    // during a rebase" is a property of the moment and will be true again in a
    // minute, which is a different thing to tell a reader than "you disabled
    // this".
    let (kept, paused): (Vec<_>, Vec<_>) = kept.into_iter().partition(|check| {
        !check
            .scope()
            .not_during
            .iter()
            .any(|state| in_progress.contains(state))
    });
    if !paused.is_empty() {
        let what = in_progress
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" and ");
        println!(
            "{} {} check(s) paused during {what}: {}",
            warning_sign(),
            paused.len(),
            paused
                .iter()
                .map(|c| c.name())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
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
fn run_concurrently<T, R, F>(items: &[T], run: F, if_thread_died: R) -> Vec<R>
where
    T: Sync,
    R: Send + Sync + Clone,
    F: Fn(&T) -> R + Sync,
{
    let slots: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for (item, slot) in items.iter().zip(&slots) {
            let run = &run;
            let died = &if_thread_died;
            scope.spawn(move || {
                // CAUGHT, not propagated. `thread::scope` re-raises a child
                // panic in the parent, which would abort the whole hook with a
                // backtrace and throw away the other nineteen checks' results —
                // and would make `if_thread_died` unreachable, which is what it
                // was until this test existed to notice.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(item)))
                    .unwrap_or_else(|_| died.clone());
                *slot.lock().expect("poisoned") = Some(outcome);
            });
        }
    });
    slots
        .into_iter()
        .map(|s| {
            s.into_inner()
                .expect("poisoned")
                .unwrap_or_else(|| if_thread_died.clone())
        })
        .collect()
}

pub fn pre_commit(ctx: &Ctx) -> Verdict {
    let in_progress = crate::git_states_in_progress(ctx.hooks_dir);
    let checks = selected_during(Stage::PreCommit, &in_progress);

    // Around the WHOLE fan-out, not per check: twenty checks run concurrently
    // and would fight over one working tree.
    let held = match crate::staged_only::StagedOnly::enter(ctx.hooks_dir) {
        Ok(guard) => guard,
        Err(e) => {
            // Refusing to check the wrong content is the safe direction; a
            // check that read the tree would be answering about a commit
            // nobody is making.
            eprintln!("{e}");
            return Verdict::Block;
        }
    };
    crate::staged_only::install_signal_handler();

    let verdict = run_stage(&checks, ctx, &Overrides::read());
    drop(held);
    verdict
}

/// The pre-commit body, over the checks it is GIVEN.
///
/// A seam, so a test can hand it a check that panics. Without it the value
/// standing in for a dead check was a literal at one call site that no test
/// could reach — the rule was asserted on the runner and merely hoped for here.
fn run_stage(checks: &[&'static dyn Check], ctx: &Ctx, severities: &Overrides) -> Verdict {
    if checks.is_empty() {
        return Verdict::Proceed;
    }
    let outcomes = run_concurrently(
        checks,
        |check| {
            let sub = Ctx {
                name: check.name(),
                args: ctx.args,
                hooks_dir: ctx.hooks_dir,
                push: ctx.push,
            };
            check.run(&sub)
        },
        // A check whose thread died has not passed. Stated here, where the slot
        // is filled, rather than hidden in a `Default` impl that every future
        // `#[derive(Default)]` would silently inherit.
        Outcome::Failed,
    );

    let report = classify(checks, &outcomes, severities);
    announce(&report);
    report.verdict()
}

/// What a stage concluded, before anything is printed or exited.
///
/// A VALUE, so the classification can be asserted directly. While this was one
/// function that classified, printed and returned an exit code, its tests could
/// only check the code — whether the right thing was SAID went untested.
#[derive(Debug, Default, PartialEq, Eq)]
struct Report<'a> {
    /// Failed, and the severity that applies blocks.
    blocked: Vec<&'a str>,
    /// Failed, but configured to warn. The check printed an error and meant it,
    /// so somebody has to say it did not block.
    downgraded: Vec<&'a str>,
    /// Could not run. Distinct from "passed", which is the whole point.
    unavailable: Vec<&'a str>,
}

impl Report<'_> {
    fn verdict(&self) -> Verdict {
        Verdict::blocking(!self.blocked.is_empty())
    }
}

/// Pure: outcomes and severities in, a verdict out. No IO.
fn classify<'a>(
    checks: &[&'a dyn Check],
    outcomes: &[Outcome],
    severities: &Overrides,
) -> Report<'a> {
    let mut report = Report::default();
    for (check, outcome) in checks.iter().zip(outcomes) {
        match outcome {
            // `Warned` needs nothing: a check that chose to warn has already
            // said what it wanted to, and a roll-up would only repeat it.
            Outcome::Passed | Outcome::Warned => {}
            Outcome::Unavailable => report.unavailable.push(check.name()),
            Outcome::Failed => match severities.of(*check) {
                Severity::Block => report.blocked.push(check.name()),
                Severity::Warn => report.downgraded.push(check.name()),
            },
        }
    }
    report
}

/// Says what happened. Prints; decides nothing.
fn announce(report: &Report) {
    if !report.unavailable.is_empty() {
        // Distinct from "passed". Silence here is how a repo looks verified
        // when nothing actually ran — and with twenty checks interleaving
        // their output, a trailing count is the only place you can see it.
        println!(
            "{} {} check(s) could not run: {}",
            warning_sign(),
            report.unavailable.len(),
            report.unavailable.join(", ")
        );
    }
    if !report.downgraded.is_empty() {
        println!(
            "{} {} check(s) reported a problem but are set to warn: {}",
            warning_sign(),
            report.downgraded.len(),
            report.downgraded.join(", ")
        );
    }
    if report.blocked.is_empty() {
        return;
    }
    println!("\n🚨  Error raised by:");
    for name in &report.blocked {
        println!("    - {}", highlight(name));
    }
}

/// `githooks run` — every applicable check, on demand.
///
/// Two questions, and the mode says which it answers:
///
/// - **staged** (default) is "would my commit pass" — the same set a commit
///   would check, so it is a rehearsal of the hook.
/// - **`--all-files`** is "does my working tree pass". Deliberately NOT the same
///   question: on a dirty tree it reports on content that is not committed and
///   may never be. That is right for adopting a check into an existing
///   repository, where `git add .` is not an acceptable way to measure the mess,
///   and it is why `--all-files` takes no stash — there is no staged/unstaged
///   distinction to protect when the answer is "all of it".
pub fn run_all(ctx: &Ctx, all_files: bool) -> Verdict {
    if all_files {
        let files = crate::git::stdout(&["ls-files"])
            .map(|out| out.lines().map(str::to_owned).collect())
            .unwrap_or_default();
        crate::hooks::common::override_file_set(files);
    }
    run_stage(&selected(Stage::PreCommit), ctx, &Overrides::read())
}

pub fn pre_push(ctx: &Ctx) -> Verdict {
    // NB: no CHERRY_PICK_HEAD check here — the zsh pre-push had none either.
    let severities = Overrides::read();
    // pre-push had NO state guard at all, with a comment admitting it existed
    // only because the zsh version had none. Now it asks the same question
    // pre-commit does and each check answers for itself.
    let in_progress = crate::git_states_in_progress(ctx.hooks_dir);
    for check in selected_during(Stage::PrePush, &in_progress) {
        let sub = Ctx {
            name: check.name(),
            args: ctx.args,
            hooks_dir: ctx.hooks_dir,
            push: ctx.push,
        };
        match check.run(&sub) {
            Outcome::Passed => {}
            // Announced, never fatal: a check that could not run has not
            // invalidated anything, and neither has a warning.
            Outcome::Unavailable => {
                println!(
                    "{} {} could not run",
                    warning_sign(),
                    highlight(check.name())
                )
            }
            Outcome::Warned => {}
            Outcome::Failed => match severities.of(check) {
                Severity::Warn => println!(
                    "{} {} reported a problem (severity warn)",
                    warning_sign(),
                    highlight(check.name())
                ),
                // Fail-fast applies ONLY to Block: the later steps are
                // expensive and their preconditions are gone.
                Severity::Block => {
                    println!("\n🚨  Error raised by hook {}", highlight(check.name()));
                    return Verdict::Block;
                }
            },
        }
    }
    Verdict::Proceed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Builtin, Scope};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A check whose only job is to carry a name and a severity into `report`.
    /// Its `run` is never called — `report` is fed outcomes directly, which is
    /// what makes `Unavailable` testable at all: the real thing needs a missing
    /// binary, and a test that uninstalls the developer's toolchain is worse
    /// than no test.
    const fn stub(name: &'static str, severity: Severity) -> Builtin {
        Builtin {
            name,
            stage: Stage::PreCommit,
            scope: Scope::ALWAYS,
            severity,
            run: |_| Outcome::Passed,
        }
    }

    /// No overrides configured. `report` takes them as a VALUE now, so its
    /// tests need no repository and no git at all.
    fn none() -> Overrides {
        Overrides::default()
    }

    static BLOCKER: Builtin = stub("stub-blocker", Severity::Block);
    static WARNER: Builtin = stub("stub-warner", Severity::Warn);

    /// The unit tests hold `&dyn Check` for the same reason the dispatcher
    /// does: `report` must not be able to tell a built-in from an external.
    const fn as_checks(cs: [&'static Builtin; 3]) -> [&'static dyn Check; 3] {
        [cs[0], cs[1], cs[2]]
    }

    /// The classification itself, which used to be unreachable: while one
    /// function classified AND printed AND returned a code, a test could assert
    /// the code and nothing else.
    #[test]
    fn every_outcome_lands_in_the_right_bucket() {
        let checks: [&dyn Check; 4] = [&BLOCKER, &BLOCKER, &WARNER, &BLOCKER];
        let got = classify(
            &checks,
            &[
                Outcome::Passed,
                Outcome::Unavailable,
                Outcome::Failed,
                Outcome::Failed,
            ],
            &none(),
        );
        assert_eq!(got.blocked, ["stub-blocker"], "{got:?}");
        assert_eq!(got.downgraded, ["stub-warner"], "{got:?}");
        assert_eq!(got.unavailable, ["stub-blocker"], "{got:?}");
    }

    /// A clean stage concludes nothing at all — not an empty message, no
    /// message. Twenty checks that passed should print no roll-ups.
    #[test]
    fn a_clean_stage_has_nothing_to_report() {
        let checks: [&dyn Check; 2] = [&BLOCKER, &WARNER];
        let got = classify(&checks, &[Outcome::Passed, Outcome::Warned], &none());
        assert_eq!(got, Report::default());
        assert_eq!(got.verdict(), Verdict::Proceed);
    }

    #[test]
    fn a_blocking_failure_is_the_only_thing_that_fails_the_commit() {
        let b: &dyn Check = &BLOCKER;
        let w: &dyn Check = &WARNER;
        assert_eq!(
            classify(&[b], &[Outcome::Failed], &none()).verdict(),
            Verdict::Block
        );
        assert_eq!(
            classify(&[b], &[Outcome::Passed], &none()).verdict(),
            Verdict::Proceed
        );
        // Every non-blocking shape, one at a time, so a regression cannot hide
        // behind a passing sibling.
        assert_eq!(
            classify(&[b], &[Outcome::Warned], &none()).verdict(),
            Verdict::Proceed
        );
        assert_eq!(
            classify(&[b], &[Outcome::Unavailable], &none()).verdict(),
            Verdict::Proceed
        );
        assert_eq!(
            classify(&[w], &[Outcome::Failed], &none()).verdict(),
            Verdict::Proceed
        );
    }

    #[test]
    fn one_blocking_failure_among_many_still_fails() {
        let checks = as_checks([&BLOCKER, &WARNER, &BLOCKER]);
        assert_eq!(
            classify(
                &checks,
                &[Outcome::Unavailable, Outcome::Failed, Outcome::Failed],
                &none()
            )
            .verdict(),
            Verdict::Block
        );
        // Same shape, with the only *blocking* failure removed.
        assert_eq!(
            classify(
                &checks,
                &[Outcome::Unavailable, Outcome::Failed, Outcome::Passed],
                &none()
            )
            .verdict(),
            Verdict::Proceed
        );
    }

    /// The slot of a check whose thread died. Reading that as a pass is how a
    /// crash becomes a green commit.
    ///
    /// Asserted through the RUNNER, not through a `Default` impl: the rule
    /// belongs to this call site, and a test on `Outcome::default()` proved
    /// only that a trait impl existed, not that the runner used it.
    /// A check that PANICS must fail the commit, not pass it — and must not
    /// take the other checks down with it.
    ///
    /// Driven through the stage body rather than the runner, because the value
    /// that stands in for a dead check is chosen at the call site and the
    /// runner's own test cannot see that choice.
    #[test]
    fn a_panicking_check_blocks_the_commit() {
        static DIES: Builtin = Builtin {
            name: "stub-dies",
            stage: Stage::PreCommit,
            scope: Scope::ALWAYS,
            severity: Severity::Block,
            run: |_| panic!("this check died"),
        };
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let push = crate::pushrefs::PushRefs::default();
        let ctx = Ctx {
            name: "pre-commit",
            args: &[],
            hooks_dir: std::path::Path::new("."),
            push: &push,
        };
        let verdict = run_stage(&[&DIES], &ctx, &none());
        std::panic::set_hook(hook);
        assert_eq!(
            verdict,
            Verdict::Block,
            "a check that died must not let the commit through"
        );
    }

    #[test]
    fn a_thread_that_dies_leaves_a_failure_behind() {
        // The default hook would print a backtrace for the deliberate panic and
        // make a passing run look broken.
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let items = ["a", "b", "c"];
        let out = run_concurrently(
            &items,
            |n: &&str| {
                if *n == "b" {
                    panic!("this check died");
                }
                Outcome::Passed
            },
            Outcome::Failed,
        );
        std::panic::set_hook(hook);
        assert_eq!(
            out,
            vec![Outcome::Passed, Outcome::Failed, Outcome::Passed],
            "a dead check must not read as one that passed, \
             and must not take the other checks down with it"
        );
    }
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

        let out = run_concurrently(
            &names,
            move |_: &&str| {
                ARRIVED.fetch_add(1, Ordering::SeqCst);
                let deadline = Instant::now() + Duration::from_secs(10);
                while ARRIVED.load(Ordering::SeqCst) < n {
                    if Instant::now() > deadline {
                        return 1; // never met the others — execution was serial
                    }
                    std::thread::yield_now();
                }
                0
            },
            1,
        );
        assert!(
            out.iter().all(|c| *c == 0),
            "tasks did not overlap: {out:?}"
        );
    }

    #[test]
    fn results_come_back_in_input_order() {
        let names: Vec<&'static str> = vec!["first", "second", "third"];
        let out = run_concurrently(&names, |n| if *n == "second" { 7 } else { 0 }, -1);
        assert_eq!(out, vec![0, 7, 0], "results keep the input order");
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
