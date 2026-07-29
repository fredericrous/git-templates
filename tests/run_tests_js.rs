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
#[test]
fn runs_the_gate_in_order_and_never_lint() {
    if missing("npm") {
        return;
    }
    let r = Repo::new();
    r.stage(
        "package.json",
        r#"{"name":"t","scripts":{"typecheck":"sh -c 'echo typecheck >> ran.txt'","lint":"sh -c 'echo lint >> ran.txt'","test:unit":"sh -c 'echo unit >> ran.txt'","test":"sh -c 'echo test >> ran.txt'"}}"#,
    );
    r.commit("chore: base");
    let base = head(&r);
    r.stage("src/a.js", "const x = 1;\n");
    r.commit("feat: a");
    assert_eq!(push_range(&r, &base, &head(&r)), 0);
    let ran = std::fs::read_to_string(r.path("ran.txt")).unwrap_or_default();
    assert_eq!(
        ran.lines().collect::<Vec<_>>(),
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
        r#"{"name":"t","scripts":{"typecheck":"exit 1","test:unit":"sh -c 'echo unit >> ran.txt'","test":"sh -c 'echo test >> ran.txt'"}}"#,
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
