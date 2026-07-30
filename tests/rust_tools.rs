//! The three Rust checks, driven through the real binary against real cargo.
//!
//! These compile a tiny crate, so they are the slowest tests here. They earn it:
//! everything interesting is in the seam between git's staged set and cargo's
//! notion of a workspace, and nothing about that seam is exercised by a unit
//! test on path strings.

mod common;
use common::{missing, Repo};
use std::io::Write;
use std::process::{Command, Stdio};

/// A minimal, dependency-free crate — no network, no registry.
fn crate_files(r: &Repo, body: &str) {
    r.stage(
        "Cargo.toml",
        "[package]\nname = \"t\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
    );
    r.stage("src/main.rs", body);
}

fn pre_push(r: &Repo, from: &str, to: &str) -> i32 {
    let line = format!("refs/heads/feat/x {to} refs/heads/feat/x {from}\n");
    let mut child = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("--hooks-dir")
        .arg(r.path(".git/hooks"))
        .arg("pre-push-cargo-test")
        .current_dir(&r.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(line.as_bytes())
        .expect("write");
    child.wait().expect("wait").code().unwrap_or(-1)
}

fn head(r: &Repo) -> String {
    String::from_utf8_lossy(&r.git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string()
}

/// The scoping guarantee that matters most: a repo with no Rust must never
/// invoke cargo, however the checks are wired.
#[test]
fn a_non_rust_repo_runs_nothing() {
    let r = Repo::new();
    r.stage("a.py", "x = 1\n");
    r.stage("README.md", "hi\n");
    assert!(r.hook("pre-commit-cargo-fmt", &[]).passed());
    assert!(r.hook("pre-commit-clippy", &[]).passed());
}

/// A `.rs` file with no `Cargo.toml` above it is not a cargo project — a loose
/// script must not make the hook try to build a workspace that doesn't exist.
#[test]
fn a_rust_file_outside_any_crate_is_skipped() {
    let r = Repo::new();
    r.stage("scripts/loose.rs", "fn main() {}\n");
    let run = r.hook("pre-commit-cargo-fmt", &[]);
    assert!(run.passed(), "{}", run.stdout);
    assert!(
        !run.says("Rust formatting"),
        "should not have run cargo at all: {}",
        run.stdout
    );
}

#[test]
fn cargo_fmt_rejects_unformatted_rust_and_accepts_clean() {
    if missing("cargo") {
        return;
    }
    let r = Repo::new();
    crate_files(&r, "fn main(){let _x=1;}\n");
    assert!(
        !r.hook("pre-commit-cargo-fmt", &[]).passed(),
        "unformatted code must fail"
    );

    let r2 = Repo::new();
    crate_files(&r2, "fn main() {\n    let _x = 1;\n}\n");
    let run = r2.hook("pre-commit-cargo-fmt", &[]);
    assert!(run.passed(), "formatted code must pass: {}", run.stdout);
    assert!(run.says("Rust formatting is clean"));
}

/// `-D warnings` is the point: an ordinary rustc warning has to fail the
/// commit, not merely print.
#[test]
fn clippy_denies_warnings() {
    if missing("cargo") {
        return;
    }
    let r = Repo::new();
    crate_files(&r, "fn main() {\n    let unused = 1;\n}\n");
    assert!(
        !r.hook("pre-commit-clippy", &[]).passed(),
        "an unused variable must fail under -D warnings"
    );

    let r2 = Repo::new();
    crate_files(&r2, "fn main() {\n    println!(\"ok\");\n}\n");
    let run = r2.hook("pre-commit-clippy", &[]);
    assert!(run.passed(), "clean code must pass: {}", run.stdout);
    assert!(run.says("Clippy passed"));
}

/// A dependency bump changes what compiles without touching a single `.rs`,
/// which is exactly when clippy is worth running.
#[test]
fn a_manifest_only_change_still_runs_clippy() {
    if missing("cargo") {
        return;
    }
    let r = Repo::new();
    crate_files(&r, "fn main() {\n    println!(\"ok\");\n}\n");
    r.commit("feat: base");
    // Stage ONLY the manifest.
    r.stage(
        "Cargo.toml",
        "[package]\nname = \"t\"\nversion = \"0.2.0\"\nedition = \"2021\"\n\n[dependencies]\n",
    );
    let run = r.hook("pre-commit-clippy", &[]);
    assert!(run.passed(), "{}", run.stdout);
    assert!(
        run.says("Clippy passed"),
        "a manifest-only change should still have run clippy: {}",
        run.stdout
    );
}

#[test]
fn cargo_test_gates_the_push() {
    if missing("cargo") {
        return;
    }
    let r = Repo::new();
    crate_files(
        &r,
        "fn main() {}\n\n#[test]\nfn passes() {\n    assert_eq!(1, 1);\n}\n",
    );
    r.commit("feat: base");
    let base = head(&r);
    r.stage(
        "src/main.rs",
        "fn main() {}\n\n#[test]\nfn fails() {\n    assert_eq!(1, 2);\n}\n",
    );
    r.commit("feat: a failing test");
    assert_ne!(
        pre_push(&r, &base, &head(&r)),
        0,
        "a failing test must abort the push"
    );
}

/// A push that touches no Rust must not pay for a workspace test run.
#[test]
fn a_docs_only_push_runs_no_tests() {
    let r = Repo::new();
    crate_files(&r, "fn main() {}\n");
    r.commit("feat: base");
    let base = head(&r);
    r.stage("README.md", "docs\n");
    r.commit("docs: readme");
    assert_eq!(pre_push(&r, &base, &head(&r)), 0);
}
