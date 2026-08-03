//! pre-push-run-tests-js. git feeds pre-push one line per ref on stdin, so the
//! cases drive the binary the same way.

mod common;
use common::{missing, Repo};
use std::io::Write;
use std::process::{Command, Stdio};

/// Feed the hook a pre-push line for the range `from..to`.
///
/// A REAL range matters: passing a zero remote-oid means "new branch", and the
/// range becomes the single root commit — on which `git diff-tree` reports
/// nothing without `--root`. So no files look changed, no gate runs, and every
/// case passes for the wrong reason. Two commits and an explicit range is what
/// the zsh suite used.
fn push_range(r: &Repo, from: &str, to: &str) -> i32 {
    let line = format!("refs/heads/main {to} refs/heads/main {from}\n");
    let mut child = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("--hooks-dir")
        .arg(r.path(".git/hooks"))
        .arg("pre-push-run-tests-js")
        .current_dir(&r.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(line.as_bytes())
        .unwrap();
    child.wait().expect("wait").code().unwrap_or(-1)
}

fn head(r: &Repo) -> String {
    String::from_utf8_lossy(&r.git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string()
}

#[test]
fn runs_a_packages_gate_and_passes_when_it_succeeds() {
    if missing("npm") {
        return;
    }
    let r = Repo::new();
    r.stage(
        "package.json",
        r#"{"name":"t","scripts":{"test":"exit 0"}}"#,
    );
    r.commit("chore: base");
    let base = head(&r);
    r.stage("src/a.js", "const x = 1;\n");
    r.commit("feat: a");
    assert_eq!(push_range(&r, &base, &head(&r)), 0);
}

#[test]
fn fails_when_the_gate_fails() {
    if missing("npm") {
        return;
    }
    let r = Repo::new();
    r.stage(
        "package.json",
        r#"{"name":"t","scripts":{"test":"exit 1"}}"#,
    );
    r.commit("chore: base");
    let base = head(&r);
    r.stage("src/a.js", "const x = 1;\n");
    r.commit("feat: a");
    assert_ne!(push_range(&r, &base, &head(&r)), 0);
}

/// typecheck → test:unit → test, cheapest first, so a type error costs seconds
/// rather than a full suite. `lint` is deliberately absent — pre-commit-lint-js
/// already lints staged files.
///
/// The recording scripts avoid `sh -c '...'`: npm runs scripts through cmd.exe
/// on Windows, where `'` does not quote. cmd would eat the `>>` redirect itself
/// and run `sh -c 'echo typecheck`, creating ran.txt EMPTY — the gate looks like
/// it ran nothing while exiting 0. `echo x>>ran.txt` means the same thing to sh
/// and to cmd. No space before `>>`: cmd would include it in the written line.
#[test]
fn runs_the_gate_in_order_and_never_lint() {
    if missing("npm") {
        return;
    }
    let r = Repo::new();
    r.stage(
        "package.json",
        r#"{"name":"t","scripts":{"typecheck":"echo typecheck>>ran.txt","lint":"echo lint>>ran.txt","test:unit":"echo unit>>ran.txt","test":"echo test>>ran.txt"}}"#,
    );
    r.commit("chore: base");
    let base = head(&r);
    r.stage("src/a.js", "const x = 1;\n");
    r.commit("feat: a");
    assert_eq!(push_range(&r, &base, &head(&r)), 0);
    let ran = std::fs::read_to_string(r.path("ran.txt")).unwrap_or_default();
    assert_eq!(
        ran.lines().map(str::trim).collect::<Vec<_>>(),
        vec!["typecheck", "unit", "test"]
    );
    assert!(
        !ran.contains("lint"),
        "lint belongs to pre-commit, not here"
    );
}

#[test]
fn a_failure_skips_the_rest_of_the_gate() {
    if missing("npm") {
        return;
    }
    let r = Repo::new();
    r.stage(
        "package.json",
        r#"{"name":"t","scripts":{"typecheck":"exit 1","test:unit":"echo unit>>ran.txt","test":"echo test>>ran.txt"}}"#,
    );
    r.commit("chore: base");
    let base = head(&r);
    r.stage("src/a.js", "const x = 1;\n");
    r.commit("feat: a");
    assert_ne!(push_range(&r, &base, &head(&r)), 0);
    assert!(
        !r.path("ran.txt").exists(),
        "nothing after the failure should run"
    );
}

/// Feed the hook a NEW-BRANCH pre-push line: the remote oid is all zeroes.
fn push_new_branch(r: &Repo, branch: &str, tip: &str) -> i32 {
    let zero = "0".repeat(tip.len());
    let line = format!("refs/heads/{branch} {tip} refs/heads/{branch} {zero}\n");
    let mut child = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("--hooks-dir")
        .arg(r.path(".git/hooks"))
        .arg("pre-push-run-tests-js")
        .current_dir(&r.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(line.as_bytes())
        .unwrap();
    child.wait().expect("wait").code().unwrap_or(-1)
}

/// A repo with a REAL bare origin holding `main`, so `--not --remotes` has a
/// remote-tracking ref to exclude against — without one, a new branch's whole
/// history is "new" and the case proves nothing about commit selection.
fn with_origin(r: &Repo) {
    let origin = r.path(".git/test-origin.git");
    r.git(&["init", "-q", "--bare", origin.to_str().expect("utf8")]);
    r.git(&["remote", "add", "origin", origin.to_str().expect("utf8")]);
    r.git(&["push", "-q", "--no-verify", "origin", "main"]);
}

/// A new branch carrying several commits must be judged by ALL of them.
///
/// The check used to build its own range: `remote_oid == zero` became the
/// single string `<tip>`, and `diff-tree <tip>` reports only what the TIP
/// changed against its parent. So a multi-commit new branch whose FIRST commit
/// breaks the JS package, with a docs commit on top, selected no package at all
/// and the push proceeded green with the suite never having run. This case
/// passes VACUOUSLY under the old code, which is exactly the bug.
#[test]
fn a_new_branch_is_judged_by_every_commit_not_just_its_tip() {
    if missing("npm") {
        return;
    }
    let r = Repo::new();
    r.stage(
        "package.json",
        r#"{"name":"t","scripts":{"test":"exit 1"}}"#,
    );
    r.commit("chore: base");
    with_origin(&r);

    r.git(&["checkout", "-q", "-b", "feat/x"]);
    r.stage("src/a.js", "const x = 1;\n");
    r.commit("feat: the js change");
    r.stage("README.md", "docs\n");
    r.commit("docs: on top of it");

    assert_ne!(
        push_new_branch(&r, "feat/x", &head(&r)),
        0,
        "the failing package was two commits back and the gate never ran"
    );
}

/// A file changed and reverted within one push still selects its package.
///
/// `diff-tree A..B` is a two-TREE compare, not a commit walk — unlike `log`,
/// where the identical syntax means the opposite. A `.js` file edited by one
/// commit and restored by a later one in the same push nets to "unchanged"
/// between the endpoints, so the package was never selected and its suite never
/// ran, even though the push plainly carries a commit that touched it.
#[test]
fn a_file_reverted_later_in_the_same_push_still_selects_its_package() {
    if missing("npm") {
        return;
    }
    let r = Repo::new();
    r.stage(
        "package.json",
        r#"{"name":"t","scripts":{"test":"exit 1"}}"#,
    );
    r.stage("src/a.js", "const x = 1;\n");
    r.commit("chore: base");
    let base = head(&r);

    r.stage("src/a.js", "const x = 2;\n");
    r.commit("feat: touch the js");
    r.stage("src/a.js", "const x = 1;\n");
    r.commit("revert: put it back");

    assert_ne!(
        push_range(&r, &base, &head(&r)),
        0,
        "the endpoints' trees are identical, but a pushed commit touched .js"
    );
}

/// A change with no JS/TS files runs nothing at all.
#[test]
fn a_non_js_change_runs_no_gate() {
    let r = Repo::new();
    r.stage(
        "package.json",
        r#"{"name":"t","scripts":{"test":"exit 1"}}"#,
    );
    r.commit("chore: seed");
    let base = head(&r);
    r.stage("README.md", "docs\n");
    r.commit("docs: readme");
    assert_eq!(push_range(&r, &base, &head(&r)), 0);
}
