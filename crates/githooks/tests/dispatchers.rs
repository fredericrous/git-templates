//! The pre-commit and pre-push dispatchers.
//!
//! They are NOT the same shape and both shapes are load-bearing — pre-commit
//! runs its checks concurrently and reports EVERY failure; pre-push runs them
//! serially and stops at the FIRST. A shared "run all" helper is the obvious
//! way to lose that distinction, so both are pinned here.
//!
//! These cases used to plant synthetic `pre-commit-aaa` shell files and watch
//! them append to a log. That scaffolding went with the file-based sub-hooks,
//! and it was always testing the harness as much as the product: it proved a
//! glob ran three scripts, not that a commit is stopped for the right reason.
//! Each case now drives a REAL check to a real verdict. The concurrency itself
//! is unit-tested by rendezvous in `dispatch::tests`, next to the runner.

mod common;
use common::Repo;
use std::io::Write;
use std::process::{Command, Stdio};

/// Content that makes `pre-commit-merge-conflict` fail. Built rather than
/// written literally, or this file trips the very check it exercises —
/// `git grep --cached` scans the whole index, including this test.
fn conflicted() -> String {
    format!(
        "{}\nours\n{}\ntheirs\n{}\n",
        "<".repeat(7),
        "=".repeat(7),
        ">".repeat(7)
    )
}

/// Content that makes `pre-commit-ban-terms` fail.
const BANNED: &str = "fit('x', () => {});\n";

/// Feed `pre-push` a ref line on stdin, as git does.
fn pre_push(r: &Repo, remote_ref: &str) -> (i32, String) {
    let line = format!("refs/heads/local aaa {remote_ref} bbb\n");
    let mut child = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("--hooks-dir")
        .arg(r.path(".git/hooks"))
        .arg("pre-push")
        .current_dir(&r.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(line.as_bytes())
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn pre_commit_passes_a_clean_tree() {
    let r = Repo::new();
    r.stage("a.txt", "fine\n");
    assert!(r.hook("pre-commit", &[]).passed());
}

/// The reason pre-commit is concurrent rather than fail-fast: fixing lint one
/// error per commit attempt is the experience this avoids.
#[test]
fn pre_commit_reports_every_failure_not_just_the_first() {
    let r = Repo::new();
    r.stage("bad.txt", &conflicted());
    r.stage("bad.ts", BANNED);
    let run = r.hook("pre-commit", &[]);
    assert!(!run.passed());
    assert!(
        run.says("pre-commit-merge-conflict"),
        "missing merge-conflict: {}",
        run.stdout
    );
    assert!(
        run.says("pre-commit-ban-terms"),
        "missing ban-terms: {}",
        run.stdout
    );
}

/// A cherry-pick replays commits that already passed these checks, so
/// re-running the CONTENT checks turns a conflict resolution into a second
/// review. That was the reason for the original guard and it still holds.
///
/// What changed: the guard skipped the whole stage, which also silenced
/// `merge-conflict` — the one check a resolution commit most needs, because
/// leaving a marker in the commit that resolves the pick is the bug. Each check
/// now declares for itself, and this one keeps running. See
/// `tests/git_state.rs`.
#[test]
fn a_cherry_pick_pauses_the_content_checks_but_not_merge_conflict() {
    let r = Repo::new();
    r.stage("x.json", "{ BROKEN\n");
    assert!(!r.hook("pre-commit", &[]).passed(), "guard: fails normally");
    std::fs::write(r.path(".git/CHERRY_PICK_HEAD"), "deadbeef\n").expect("write");
    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "content checks must pause:\n{}", run.output());
    assert!(run.says("paused during a cherry-pick"), "{}", run.output());

    // …and the marker check is not among them.
    let r = Repo::new();
    r.stage("bad.txt", &conflicted());
    std::fs::write(r.path(".git/CHERRY_PICK_HEAD"), "deadbeef\n").expect("write");
    assert!(
        !r.hook("pre-commit", &[]).passed(),
        "a conflict marker must still block the resolution commit"
    );
}

/// A SHORT NAME names its check. This used to work by substring reach; it now
/// works because the short name is one of the three things that name a check,
/// which is the same convenience without the collateral.
#[test]
fn a_short_name_disables_its_check() {
    let r = Repo::new();
    r.stage("bad.txt", &conflicted());
    assert!(!r.hook("pre-commit", &[]).passed(), "guard");
    r.git(&["config", "--add", "hook.skip", "merge-conflict"]);
    assert!(r.hook("pre-commit", &[]).passed(), "skip should disable it");
}

/// And the full id names it too, so both spellings a reader might copy out of
/// the dashboard work.
#[test]
fn a_full_id_disables_its_check() {
    let r = Repo::new();
    r.stage("bad.txt", &conflicted());
    r.git(&["config", "--add", "hook.skip", "pre-commit-merge-conflict"]);
    assert!(r.hook("pre-commit", &[]).passed());
}

#[test]
fn pre_push_allows_an_ordinary_branch() {
    let r = Repo::new();
    r.stage("a.txt", "x\n");
    r.commit("feat: a");
    // The LOCAL branch has to satisfy branch-pattern too — a fresh repo is on
    // `main`, which that check rejects by design.
    r.git(&["checkout", "-q", "-b", "feat/x"]);
    let (code, out) = pre_push(&r, "refs/heads/feat/x");
    assert_eq!(code, 0, "{out}");
}

/// pre-push is fail-fast, and branch-protect is deliberately FIRST: the
/// cheapest and most decisive step. Nothing after it should have run.
#[test]
fn pre_push_stops_at_the_first_failure() {
    let r = Repo::new();
    r.stage("a.txt", "x\n");
    r.commit("feat: a");
    r.git(&["checkout", "-q", "-b", "feat/x"]);
    let (code, out) = pre_push(&r, "refs/heads/main");
    assert_ne!(code, 0);
    assert!(
        out.contains("forbidden"),
        "expected the protect message: {out}"
    );
    assert!(
        out.contains("pre-push-branch-protect"),
        "the failing check is named singularly: {out}"
    );
    assert!(
        !out.contains("upstream"),
        "pull-rebase ran after a failure — fail-fast is broken: {out}"
    );
}

/// An unknown hook name is still loud: it exits 2 rather than passing silently.
#[test]
fn an_unknown_hook_exits_two() {
    let r = Repo::new();
    let run = r.hook("pre-commit-not-a-hook", &[]);
    assert_eq!(run.code, 2);
    assert!(run.says("unknown hook"));
}

/// A skip is otherwise invisible at exactly the moment it matters: with
/// `hook.skip` set, a commit printed a wall of green ticks and no hint that a
/// check had been disabled. The developer sees a clean run and concludes they
/// are covered.
#[test]
fn skipped_checks_are_announced_by_name() {
    let r = Repo::new();
    r.stage("a.txt", "x\n");
    r.git(&["config", "--add", "hook.skip", "merge-conflict"]);
    let run = r.hook("pre-commit", &[]);
    assert!(run.passed());
    assert!(
        run.says("pre-commit-merge-conflict"),
        "the skipped check must be named: {}",
        run.stdout
    );
    assert!(run.says("skipped by"), "{}", run.stdout);
}

/// And silent otherwise — a line on every ordinary commit would be noise, and
/// noise is how a warning stops being read.
#[test]
fn no_skips_means_no_extra_output() {
    let r = Repo::new();
    r.stage("a.txt", "x\n");
    let run = r.hook("pre-commit", &[]);
    assert!(!run.says("skipped by"), "{}", run.stdout);
}

/// A one-letter skip used to disable all twenty checks, because matching was
/// `check.contains(skip)` and every name contains an `e`. The old doc comment
/// called that "a sharp edge, not a bug" and the dispatcher announced the
/// damage on every commit.
///
/// It now reaches nothing at all, so there is no damage to announce. A value
/// names a check only by its full id, its trigger, or its short name.
#[test]
fn a_one_letter_skip_now_reaches_nothing() {
    let r = Repo::new();
    r.stage("a.txt", "x\n");
    r.git(&["config", "--add", "hook.skip", "e"]);
    let run = r.hook("pre-commit", &[]);
    assert!(
        !run.says("skipped by"),
        "`e` should suppress nothing at all: {}",
        run.stdout
    );
    assert!(run.says("No merge conflict detected"), "{}", run.stdout);
}

/// A TRIGGER, though, still disables its whole stage — deliberately now, rather
/// than by accident of substring reach — and still says exactly what it cost.
#[test]
fn a_trigger_skip_disables_its_stage_and_says_so() {
    let r = Repo::new();
    r.stage("a.txt", "x\n");
    r.git(&["config", "--add", "hook.skip", "pre-commit"]);
    let run = r.hook("pre-commit", &[]);
    assert!(
        run.says("15 checks skipped"),
        "a trigger disables its stage: {}",
        run.stdout
    );
    // Named, not just counted: a number alone does not tell you what you lost.
    assert!(run.says("pre-commit-ban-terms"), "{}", run.stdout);
}

/// pre-push announces its own, separately — the two dispatchers have different
/// check lists and run at different moments.
#[test]
fn pre_push_announces_its_skips_too() {
    let r = Repo::new();
    r.stage("a.txt", "x\n");
    r.commit("feat: a");
    r.git(&["checkout", "-q", "-b", "feat/x"]);
    r.git(&["config", "--add", "hook.skip", "branch-protect"]);
    let (_, out) = pre_push(&r, "refs/heads/feat/x");
    assert!(
        out.contains("pre-push-branch-protect"),
        "expected the skip to be named at push time: {out}"
    );
}

/// `NO_COLOR` must actually work, not merely be documented.
///
/// The fleet spec asserted this property for several PRs while nothing read the
/// variable. Colour is now base ANSI so the terminal's own theme decides the
/// hue, and this checks the off switch exists — the glyphs carry the meaning
/// regardless, which is what makes turning colour off safe.
#[test]
fn no_color_suppresses_every_escape_sequence() {
    let r = Repo::new();
    r.stage("a.txt", "x\n");

    let with_colour = r.hook("pre-commit", &[]);
    assert!(
        with_colour.stdout.contains('\u{1b}'),
        "colour is on by default: {:?}",
        with_colour.stdout
    );

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("--hooks-dir")
        .arg(r.path(".git/hooks"))
        .arg("pre-commit")
        .current_dir(&r.dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("run");
    let plain = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        !plain.contains('\u{1b}'),
        "NO_COLOR must leave no escape sequences: {plain:?}"
    );
    // And the screen stays readable without them.
    assert!(
        plain.contains('✓'),
        "the glyph carries the meaning: {plain}"
    );
}

/// A terminal that cannot render SGR gets none either.
#[test]
fn term_dumb_suppresses_colour() {
    let r = Repo::new();
    r.stage("a.txt", "x\n");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("--hooks-dir")
        .arg(r.path(".git/hooks"))
        .arg("pre-commit")
        .current_dir(&r.dir)
        .env("TERM", "dumb")
        .output()
        .expect("run");
    assert!(!String::from_utf8_lossy(&out.stdout).contains('\u{1b}'));
}

/// `githooks list` answers "would this run here, and if not why", which was
/// previously a code-reading exercise. It lives in the hook binary because that
/// is the one installed everywhere and the question is about the repo you are
/// standing in.
#[test]
fn list_reports_what_would_run_here() {
    let r = Repo::new();
    r.stage("Cargo.toml", "[package]\nname=\"t\"\n");
    r.stage("src/main.rs", "fn main() {}\n");
    r.commit("feat: a rust repo");

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("list")
        // Asserting on the LAYOUT, so ask for it without SGR rather than
        // matching around escape sequences.
        .env("NO_COLOR", "1")
        .current_dir(&r.dir)
        .output()
        .expect("run");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();

    // The trigger is a HEADING, and each row under it is the short name. It
    // used to be repeated on all twenty rows, which pushed the reason — the one
    // part that differs per row — eleven columns to the right.
    assert!(
        text.lines().any(|l| l == "pre-commit"),
        "the trigger heads its section: {text}"
    );
    let clippy = text.lines().find(|l| l.contains("clippy")).unwrap_or("");
    let ruff = text.lines().find(|l| l.contains("ruff")).unwrap_or("");
    assert!(
        !clippy.contains("pre-commit-"),
        "the heading already said the trigger: {clippy}"
    );
    // A rust repo: clippy runs, ruff is INERT — and inert is not failure.
    assert!(clippy.contains('●'), "clippy should run here: {clippy}");
    assert!(ruff.contains('○'), "ruff should be inert here: {ruff}");
    assert!(ruff.contains(".py"), "and say what it would need: {ruff}");
}

/// A skipped check is shown as skipped, not as inert — they mean different
/// things and the glyphs must not be interchangeable.
#[test]
fn list_distinguishes_skipped_from_inert() {
    let r = Repo::new();
    r.stage("Cargo.toml", "[package]\nname=\"t\"\n");
    r.stage("src/main.rs", "fn main() {}\n");
    r.commit("feat: a rust repo");
    r.git(&["config", "--add", "hook.skip", "pre-commit-clippy"]);

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("list")
        .current_dir(&r.dir)
        .output()
        .expect("run");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let clippy = text.lines().find(|l| l.contains("clippy")).unwrap_or("");
    assert!(clippy.contains('⊘'), "expected the skip glyph: {clippy}");
    assert!(clippy.contains("hook.skip"), "{clippy}");
}
