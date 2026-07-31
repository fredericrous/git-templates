//! Checks that pause while a git operation is part-way through.
//!
//! This replaces one hard-coded `CHERRY_PICK_HEAD` test in one dispatcher —
//! the other carried a comment admitting it had no guard "because the zsh
//! pre-push had none either", which is history rather than a decision.

mod common;
use common::Repo;

fn conflicted() -> String {
    format!(
        "{}\nours\n{}\ntheirs\n{}\n",
        "<".repeat(7),
        "=".repeat(7),
        ">".repeat(7)
    )
}

/// Content checks cannot say anything useful mid-operation: half the tree is
/// somebody else's work and you cannot fix it from in here.
#[test]
fn content_checks_pause_during_a_merge() {
    let r = Repo::new();
    r.stage("x.json", "{ BROKEN\n");
    assert!(!r.hook("pre-commit", &[]).passed(), "baseline");

    std::fs::write(r.path(".git/MERGE_HEAD"), "deadbeef\n").expect("write");
    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "should have paused:\n{}", run.output());
    assert!(run.says("paused during a merge"), "{}", run.output());
    assert!(
        run.says("pre-commit-lint-json-yaml"),
        "must name what paused:\n{}",
        run.output()
    );
}

/// Every operation, not just the one somebody hit first.
#[test]
fn every_marker_is_recognised() {
    for (marker, word) in [
        ("MERGE_HEAD", "a merge"),
        ("CHERRY_PICK_HEAD", "a cherry-pick"),
        ("REVERT_HEAD", "a revert"),
        ("REBASE_HEAD", "a rebase"),
    ] {
        let r = Repo::new();
        r.stage("x.json", "{ BROKEN\n");
        std::fs::write(r.path(".git").join(marker), "deadbeef\n").expect("write");
        let run = r.hook("pre-commit", &[]);
        assert!(run.passed(), "{marker} did not pause:\n{}", run.output());
        assert!(run.says(word), "{marker} misnamed:\n{}", run.output());
    }
}

/// `rebase-merge` is a DIRECTORY, not a file. `Path::exists` covers both, and
/// an implementation that only tested for files would miss the common case —
/// an interactive rebase.
#[test]
fn an_interactive_rebase_directory_counts() {
    let r = Repo::new();
    r.stage("x.json", "{ BROKEN\n");
    std::fs::create_dir_all(r.path(".git/rebase-merge")).expect("mkdir");
    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "{}", run.output());
    assert!(run.says("a rebase"), "{}", run.output());
}

/// The behaviour change worth arguing for: the old guard skipped the WHOLE
/// pre-commit stage during a cherry-pick, which silenced the one check you most
/// want on a resolution commit. Leaving a conflict marker in the commit that
/// resolves a merge is the bug.
#[test]
fn merge_conflict_still_runs_during_a_merge() {
    let r = Repo::new();
    r.stage("bad.txt", &conflicted());
    std::fs::write(r.path(".git/MERGE_HEAD"), "deadbeef\n").expect("write");

    let run = r.hook("pre-commit", &[]);
    assert!(
        !run.passed(),
        "a conflict marker in a resolution commit must still block:\n{}",
        run.output()
    );
    assert!(run.says("conflict"), "{}", run.output());
}

/// Nothing in progress means nothing paused, and no noise about it.
#[test]
fn a_normal_commit_says_nothing_about_git_state() {
    let r = Repo::new();
    r.stage("a.txt", "hello\n");
    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "{}", run.output());
    assert!(!run.says("paused"), "{}", run.output());
}
