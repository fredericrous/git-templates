//! `githooks agents-md` — writing/updating the marker-delimited block in
//! `AGENTS.md`, and `--check` for CI drift detection.
//!
//! The core marker-detection logic (`desired_file_content`) is unit-tested in
//! `githooks-runtime`; these drive the actual binary and a real file on disk,
//! the way `pull_rebase.rs`'s suite drives `pre-push-pull-rebase` rather than
//! re-testing `divergence` through the CLI.

mod common;
use common::Repo;
use std::process::Command;

fn run_agents_md(r: &Repo, extra: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("agents-md")
        .args(extra)
        .current_dir(&r.dir)
        .output()
        .expect("run");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn creates_a_new_file() {
    let r = Repo::new();
    r.commit("init");
    assert!(!r.path("AGENTS.md").exists());

    let (code, _, _) = run_agents_md(&r, &[]);
    assert_eq!(code, 0);
    let content = std::fs::read_to_string(r.path("AGENTS.md")).expect("read");
    assert!(content.contains("<!-- githooks:start -->"));
    assert!(content.contains("githooks list --json"));
}

#[test]
fn preserves_unrelated_existing_content_when_appending() {
    let r = Repo::new();
    r.commit("init");
    r.write("AGENTS.md", "# My Project\n\nSome existing notes.\n");

    let (code, _, _) = run_agents_md(&r, &[]);
    assert_eq!(code, 0);
    let content = std::fs::read_to_string(r.path("AGENTS.md")).expect("read");
    assert!(content.starts_with("# My Project\n\nSome existing notes.\n"));
    assert!(content.contains("<!-- githooks:start -->"));
}

#[test]
fn replaces_only_the_marked_block() {
    let r = Repo::new();
    r.commit("init");
    r.write(
        "AGENTS.md",
        "before\n\n<!-- githooks:start -->\nSTALE CONTENT\n<!-- githooks:end -->\n\nafter\n",
    );

    let (code, _, _) = run_agents_md(&r, &[]);
    assert_eq!(code, 0);
    let content = std::fs::read_to_string(r.path("AGENTS.md")).expect("read");
    assert!(content.starts_with("before\n\n"));
    assert!(content.ends_with("\n\nafter\n"));
    assert!(!content.contains("STALE CONTENT"));
}

#[test]
fn check_reports_not_present_and_exits_zero() {
    let r = Repo::new();
    r.commit("init");
    let (code, stdout, _) = run_agents_md(&r, &["--check"]);
    assert_eq!(code, 0, "{stdout}");
    assert!(stdout.contains("not present"), "{stdout}");
}

#[test]
fn check_detects_drift() {
    let r = Repo::new();
    r.commit("init");
    r.write(
        "AGENTS.md",
        "<!-- githooks:start -->\nSTALE\n<!-- githooks:end -->\n",
    );
    let (code, _, stderr) = run_agents_md(&r, &["--check"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("drifted"), "{stderr}");
}

#[test]
fn check_matches_after_a_write() {
    let r = Repo::new();
    r.commit("init");
    let (write_code, _, _) = run_agents_md(&r, &[]);
    assert_eq!(write_code, 0);
    let (check_code, stdout, _) = run_agents_md(&r, &["--check"]);
    assert_eq!(check_code, 0, "{stdout}");
    assert!(stdout.contains("up to date"), "{stdout}");
}

#[test]
fn respects_a_custom_path() {
    let r = Repo::new();
    r.commit("init");
    let (code, stdout, _) = run_agents_md(&r, &["--path", "docs/AGENTS.md"]);
    assert_eq!(code, 0, "{stdout}");
    assert!(
        std::fs::read_to_string(r.path("docs/AGENTS.md")).is_ok(),
        "the custom path must be honoured, not the default AGENTS.md"
    );
    assert!(!r.path("AGENTS.md").exists());
}
