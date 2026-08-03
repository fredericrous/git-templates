//! `githooks run` — the checks, on demand.
//!
//! Two questions, and the tests pin that they are different. Staged is "would
//! my commit pass"; `--all-files` is "does my working tree pass", which on a
//! dirty tree reports on content that is not committed and may never be.

mod common;
use common::Repo;
use std::process::Command;

fn run(r: &Repo, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("run")
        .args(args)
        .current_dir(&r.dir)
        .output()
        .expect("githooks run");
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// The default is a rehearsal of the commit: the same set a commit would check.
#[test]
fn run_checks_the_staged_set() {
    let r = Repo::new();
    r.stage("good.json", "{ \"a\": 1 }\n");
    r.commit("chore: seed");
    // Committed but NOT staged now — invisible to the staged question.
    r.write("bad.json", "{ BROKEN\n");
    r.stage("good.json", "{ \"a\": 2 }\n");

    let (code, out) = run(&r, &[]);
    assert_eq!(code, 0, "the staged set is valid:\n{out}");
    assert!(
        !out.contains("bad.json"),
        "it looked outside the index:\n{out}"
    );
}

/// `--all-files` asks the other question, and finds what staging hid.
#[test]
fn all_files_sees_what_is_not_staged() {
    let r = Repo::new();
    r.stage("good.json", "{ \"a\": 1 }\n");
    r.commit("chore: seed");
    r.stage("bad.json", "{ BROKEN\n");
    r.commit("chore: add bad");

    let (code, out) = run(&r, &["--all-files"]);
    assert_ne!(code, 0, "should have found the broken file:\n{out}");
    assert!(out.contains("bad.json"), "{out}");
}

/// The two modes must actually differ, or one of them is decoration.
#[test]
fn the_two_modes_answer_different_questions() {
    let r = Repo::new();
    r.stage("good.json", "{ \"a\": 1 }\n");
    r.commit("chore: seed");
    r.stage("bad.json", "{ BROKEN\n");
    r.commit("chore: add bad");
    // Nothing staged at all now.
    let (staged, _) = run(&r, &[]);
    let (all, _) = run(&r, &["--all-files"]);
    assert_eq!(staged, 0, "nothing staged means nothing to judge");
    assert_ne!(all, 0, "but the tree still holds a broken file");
}

/// One check by name, which is how you adopt a single check.
#[test]
fn a_named_check_runs_alone() {
    let r = Repo::new();
    r.stage("a.txt", "hello\n");
    let (code, out) = run(&r, &["pre-commit-merge-conflict"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("merge"), "the named check did not run:\n{out}");
    assert!(
        !out.contains("package.json"),
        "it ran the whole stage instead:\n{out}"
    );
}

/// A typo must not look like a clean run.
#[test]
fn an_unknown_check_is_an_error_not_a_pass() {
    let r = Repo::new();
    let (code, out) = run(&r, &["pre-commit-not-a-thing"]);
    assert_eq!(code, 2, "{out}");
    assert!(
        out.contains("githooks list"),
        "should point somewhere:\n{out}"
    );
}

/// A pre-push check run standalone has no real push to read refs from. With
/// no upstream configured either, there is nothing to synthesize one from —
/// that must be a clear error, not a silent pass with an empty ref list
/// (which is what an unset `PushRefs` looked like to `branch_protect`, since
/// `githooks/tests/common::Repo::git` always closes the child's stdin).
#[test]
fn a_named_pre_push_check_with_no_upstream_errors_instead_of_passing_empty() {
    let r = Repo::new();
    r.stage("a.txt", "x\n");
    r.commit("init");
    let (code, out) = run(&r, &["pre-push-branch-protect"]);
    assert_eq!(code, 2, "should refuse, not silently pass:\n{out}");
    assert!(out.contains("upstream"), "{out}");
}

/// With an upstream configured, the standalone run synthesizes the same ref
/// a real push would carry — proven here by a check whose verdict depends on
/// which branch that ref names.
#[test]
fn a_named_pre_push_check_uses_the_upstream_as_its_push_ref() {
    let r = Repo::new();
    r.stage("a.txt", "x\n");
    r.commit("init");
    let origin = r.path(".git/test-origin.git");
    r.git(&["init", "-q", "--bare", origin.to_str().unwrap()]);
    r.git(&["remote", "add", "origin", origin.to_str().unwrap()]);
    r.git(&["push", "-q", "--no-verify", "-u", "origin", "main"]);

    // `main` is protected, and this push targets `main` — must be refused.
    let (code, out) = run(&r, &["pre-push-branch-protect"]);
    assert_ne!(
        code, 0,
        "a synthesized push to main must be blocked:\n{out}"
    );
    assert!(out.contains("main"), "{out}");
}
