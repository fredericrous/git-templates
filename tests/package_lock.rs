//! pre-commit-package-lock, ported from its zsh suite.

mod common;
use common::Repo;

#[test]
fn passes_when_both_are_staged() {
    let r = Repo::new();
    r.stage("package.json", "{}\n");
    r.stage("package-lock.json", "{}\n");
    assert!(r.hook("pre-commit-package-lock", &[]).passed());
}

#[test]
fn rejects_a_lockfile_staged_without_its_package_json() {
    let r = Repo::new();
    r.stage("package-lock.json", "{}\n");
    let run = r.hook("pre-commit-package-lock", &[]);
    assert!(!run.passed());
    assert!(run.says("without its package.json"));
}

/// The `.git/hooks/package.json` type-marker case: no lockfile beside it, so it
/// is not an npm project and must demand nothing.
#[test]
fn a_package_json_with_no_lockfile_on_disk_demands_nothing() {
    let r = Repo::new();
    r.stage("package.json", "{}\n");
    assert!(r.hook("pre-commit-package-lock", &[]).passed());
}

/// In a monorepo one project's lockfile does not satisfy another's.
#[test]
fn scoping_is_per_directory() {
    let r = Repo::new();
    r.write("package-lock.json", "{}\n"); // root lock, on disk only
    r.stage("apps/web/package.json", "{}\n");
    // no lock beside apps/web → nothing demanded
    assert!(r.hook("pre-commit-package-lock", &[]).passed());
}
