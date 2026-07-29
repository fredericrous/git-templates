//! pre-push-branch-pattern, ported from tests/pre-push-branch-pattern.test.zsh.

mod common;
use common::Repo;

/// The hook allows any name when the remote has no branches yet, so a real
/// (bare) origin with one branch is needed or every case short-circuits to a
/// pass. The zsh suite learned this the hard way; the setup is preserved.
fn repo_with_origin() -> Repo {
    let r = Repo::new();
    r.git(&["commit", "-q", "--allow-empty", "--no-verify", "-m", "init"]);
    let origin = r.path(".fake-origin.git");
    r.git(&["init", "-q", "--bare", origin.to_str().unwrap()]);
    r.git(&["remote", "add", "origin", origin.to_str().unwrap()]);
    r.git(&["push", "-q", "--no-verify", "origin", "HEAD:main"]);
    r
}

#[test]
fn passes_when_head_cannot_be_resolved() {
    let r = repo_with_origin();
    let saved = String::from_utf8_lossy(&r.git(&["symbolic-ref", "HEAD"]).stdout)
        .trim()
        .to_string();
    r.write(".git/HEAD", "ref: refs/heads/__nonexistent__\n");
    assert!(r.hook("pre-push-branch-pattern", &[]).passed());
    r.git(&["symbolic-ref", "HEAD", &saved]);
}

#[test]
fn passes_when_the_branch_is_already_on_the_server() {
    let r = repo_with_origin();
    r.git(&["branch", "-m", "off-pattern"]);
    r.git(&["update-ref", "refs/remotes/origin/off-pattern", "HEAD"]);
    assert!(r.hook("pre-push-branch-pattern", &[]).passed());
}

#[test]
fn rejects_a_name_that_does_not_conform() {
    let r = repo_with_origin();
    r.git(&["branch", "-m", "off-pattern"]);
    assert!(!r.hook("pre-push-branch-pattern", &[]).passed());
}

#[test]
fn accepts_a_conforming_name() {
    let r = repo_with_origin();
    r.git(&["branch", "-m", "feat/0-test"]);
    assert!(r.hook("pre-push-branch-pattern", &[]).passed());
}

#[test]
fn dots_are_allowed_under_chore_only() {
    let r = repo_with_origin();
    r.git(&["branch", "-m", "chore/duro-1.50.50"]);
    assert!(r.hook("pre-push-branch-pattern", &[]).passed());
    r.git(&["branch", "-m", "feat/duro-1.50.50"]);
    assert!(!r.hook("pre-push-branch-pattern", &[]).passed());
}

#[test]
fn a_name_without_a_type_prefix_is_rejected() {
    let r = repo_with_origin();
    r.git(&["branch", "-m", "duro-1.50.50"]);
    assert!(!r.hook("pre-push-branch-pattern", &[]).passed());
}

/// The vocabulary fix (#30): these were all rejected while being valid commit
/// types. Driven through the real hook, not just the predicate.
#[test]
fn the_previously_rejected_prefixes_are_accepted() {
    let r = repo_with_origin();
    for b in [
        "docs/x",
        "refactor/y",
        "perf/z",
        "build/w",
        "style/v",
        "revert/u",
        "add/t",
        "remove/s",
    ] {
        r.git(&["branch", "-m", b]);
        assert!(r.hook("pre-push-branch-pattern", &[]).passed(), "{b}");
    }
}
