//! pre-push suites should answer about the commits being pushed.
//!
//! They took their file set from the pushed refs — correct — and then ran with
//! `current_dir` set to the developer's working tree, so the suite could pass
//! on an uncommitted fix or fail on an uncommitted experiment. The same gap as
//! the pre-commit one, from the other end.

mod common;
use common::Repo;
use std::io::Write;
use std::process::{Command, Stdio};

fn pre_push(r: &Repo, tip: &str, base: &str) -> (i32, String) {
    let line = format!("refs/heads/feat/x {tip} refs/heads/feat/x {base}\n");
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
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn rev(r: &Repo, what: &str) -> String {
    String::from_utf8_lossy(&r.git(&["rev-parse", what]).stdout)
        .trim()
        .to_string()
}

/// A repo whose committed suite FAILS and whose working tree PASSES. The two
/// modes must disagree about it, or one of them is not doing what it says.
fn diverging(r: &Repo) -> (String, String) {
    r.stage(
        "package.json",
        "{ \"name\": \"x\", \"scripts\": { \"test\": \"node check.js\" } }\n",
    );
    r.stage("check.js", "process.exit(0);\n");
    r.commit("chore: seed");
    let base = rev(r, "HEAD");
    r.git(&["checkout", "-q", "-b", "feat/x"]);

    r.stage("check.js", "process.exit(1);\n");
    r.commit("feat: break the suite");
    let tip = rev(r, "HEAD");

    // Fixed on disk, deliberately not committed.
    r.write("check.js", "process.exit(0);\n");
    (tip, base)
}

/// Today's behaviour is kept — and the actual bug was that nobody knew it was
/// the behaviour, so it now says so.
#[test]
fn by_default_it_tests_the_working_tree_and_says_so() {
    let r = Repo::new();
    let (tip, base) = diverging(&r);

    let (code, out) = pre_push(&r, &tip, &base);
    assert_eq!(code, 0, "the working tree passes:\n{out}");
    assert!(
        out.contains("WORKING TREE"),
        "it must say what it tested:\n{out}"
    );
}

/// Opted in, it catches what is actually being pushed.
#[test]
fn opted_in_it_tests_the_pushed_commits() {
    let r = Repo::new();
    let (tip, base) = diverging(&r);
    r.git(&["config", "githooks.testPushedTree", "true"]);

    let (code, out) = pre_push(&r, &tip, &base);
    assert_ne!(
        code, 0,
        "the pushed commit is broken and this let it through:\n{out}"
    );
    assert!(
        !out.contains("WORKING TREE"),
        "it should not be warning about the tree it did not use:\n{out}"
    );
}

/// The developer's tree is theirs. A push must not touch it, and must not leave
/// a worktree registered behind either.
#[test]
fn the_working_tree_is_untouched_and_nothing_is_left_behind() {
    let r = Repo::new();
    let (tip, base) = diverging(&r);
    r.git(&["config", "githooks.testPushedTree", "true"]);

    let before = std::fs::read_to_string(r.path("check.js")).expect("read");
    let (_, _) = pre_push(&r, &tip, &base);

    assert_eq!(
        std::fs::read_to_string(r.path("check.js")).expect("read"),
        before,
        "the push modified the working tree"
    );
    let worktrees = r.git(&["worktree", "list"]);
    let listed = String::from_utf8_lossy(&worktrees.stdout);
    assert_eq!(
        listed.lines().count(),
        1,
        "a worktree outlived the push:\n{listed}"
    );
}
