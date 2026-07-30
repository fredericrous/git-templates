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

/// A cherry-pick replays commits that already passed these checks; re-running
/// them turns a conflict resolution into a second review.
#[test]
fn pre_commit_skips_everything_during_a_cherry_pick() {
    let r = Repo::new();
    r.stage("bad.txt", &conflicted());
    assert!(!r.hook("pre-commit", &[]).passed(), "guard: fails normally");
    std::fs::write(r.path(".git/CHERRY_PICK_HEAD"), "deadbeef\n").expect("write");
    assert!(
        r.hook("pre-commit", &[]).passed(),
        "must skip mid-cherry-pick"
    );
}

/// `hook.skip` takes SUBSTRINGS, not exact names — it matched paths before and
/// must keep behaving the same now that it matches check names.
#[test]
fn hook_skip_is_a_substring_match() {
    let r = Repo::new();
    r.stage("bad.txt", &conflicted());
    assert!(!r.hook("pre-commit", &[]).passed(), "guard");
    r.git(&["config", "--add", "hook.skip", "merge-conflict"]);
    assert!(r.hook("pre-commit", &[]).passed(), "skip should disable it");
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
