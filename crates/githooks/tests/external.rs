//! Checks a repository declares for itself, in `.githooks.conf`.
//!
//! Unit tests cover the parser. These cover the part a parser cannot: that a
//! line in a committed file actually causes a command to run, actually blocks a
//! commit, and is governed by the same `hook.skip` and severity machinery as a
//! built-in — a repository that can add a check it cannot disable would be a
//! worse deal than not being able to add one.

mod common;
use common::Repo;
use std::io::Write;
use std::process::{Command, Stdio};

/// A committed script that records having run and exits how it is told.
///
/// Deliberately not `sh -c`: the manifest executes argv directly, and a fixture
/// that reached for a shell would be testing a shell rather than the feature.
#[cfg(unix)]
fn probe(r: &Repo, name: &str, exit: i32) {
    let body = format!("#!/bin/sh\necho probe-{name} ran\nexit {exit}\n");
    r.stage(name, &body);
    let p = r.path(name);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

#[cfg(not(unix))]
fn probe(r: &Repo, name: &str, exit: i32) {
    // `.cmd` is directly executable on Windows without a shebang.
    let body = format!("@echo probe-{name} ran\r\n@exit /b {exit}\r\n");
    r.stage(name, &body);
}

#[cfg(unix)]
const PROBE: &str = "probe.sh";
#[cfg(not(unix))]
const PROBE: &str = "probe.cmd";

fn manifest(r: &Repo, body: &str) {
    r.stage(".githooks.conf", body);
}

fn pre_push(r: &Repo) -> (i32, String) {
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
        .write_all(b"refs/heads/feat/x aaa refs/heads/feat/x bbb\n")
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

/// A push of `remote..local`, as git presents it on stdin.
fn push_range(r: &Repo, remote: &str, local: &str) -> (i32, String) {
    let line = format!("refs/heads/feat/x {local} refs/heads/feat/x {remote}\n");
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

#[test]
fn a_declared_check_runs_and_can_block_the_commit() {
    let r = Repo::new();
    probe(&r, PROBE, 1);
    manifest(
        &r,
        &format!("pre-commit  shellcheck  *  block  ./{PROBE}\n"),
    );

    let run = r.hook("pre-commit", &[]);
    assert!(
        run.says("probe-"),
        "the command never ran:\n{}",
        run.output()
    );
    assert!(!run.passed(), "a blocking failure must stop the commit");
    assert!(run.says("shellcheck"), "by name:\n{}", run.output());
}

#[test]
fn a_declared_check_that_passes_lets_the_commit_through() {
    let r = Repo::new();
    probe(&r, PROBE, 0);
    manifest(
        &r,
        &format!("pre-commit  shellcheck  *  block  ./{PROBE}\n"),
    );

    let run = r.hook("pre-commit", &[]);
    assert!(run.says("probe-"), "{}", run.output());
    assert!(run.passed(), "{}", run.output());
}

/// The severity column is the author's choice, and it has to mean the same
/// thing it means for a built-in.
#[test]
fn a_declared_check_can_choose_not_to_block() {
    let r = Repo::new();
    probe(&r, PROBE, 1);
    manifest(&r, &format!("pre-commit  smoke  *  warn  ./{PROBE}\n"));

    let run = r.hook("pre-commit", &[]);
    assert!(run.says("probe-"), "it must still RUN:\n{}", run.output());
    assert!(run.passed(), "warn must not block:\n{}", run.output());
    assert!(run.says("set to warn"), "{}", run.output());
}

/// Scope is evaluated against the staged files, exactly as a built-in's is.
#[test]
fn scope_keeps_a_declared_check_out_of_unrelated_commits() {
    let r = Repo::new();
    probe(&r, PROBE, 1);
    manifest(
        &r,
        &format!("pre-commit  shellcheck  *.rs  block  ./{PROBE}\n"),
    );

    // Nothing staged ends in `.rs` — the probe and the manifest do not.
    let run = r.hook("pre-commit", &[]);
    assert!(
        !run.says("probe-"),
        "it ran on a commit it does not apply to:\n{}",
        run.output()
    );
    assert!(run.passed());

    r.stage("src/main.rs", "fn main() {}\n");
    let run = r.hook("pre-commit", &[]);
    assert!(run.says("probe-"), "it must fire now:\n{}", run.output());
}

/// A repository that can add a check it cannot disable is a worse deal than one
/// that cannot add checks at all.
#[test]
fn hook_skip_and_severity_govern_a_declared_check() {
    let r = Repo::new();
    probe(&r, PROBE, 1);
    manifest(
        &r,
        &format!("pre-commit  shellcheck  *  block  ./{PROBE}\n"),
    );
    assert!(!r.hook("pre-commit", &[]).passed(), "baseline");

    r.git(&["config", "githooks.severity.shellcheck", "warn"]);
    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "severity override ignored:\n{}", run.output());
    assert!(run.says("probe-"), "downgraded, not disabled");

    r.git(&["config", "--unset", "githooks.severity.shellcheck"]);
    r.git(&["config", "hook.skip", "shellcheck"]);
    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "{}", run.output());
    assert!(
        !run.says("probe-"),
        "a skipped check must not run:\n{}",
        run.output()
    );
    assert!(
        run.says("skipped by"),
        "and the skip must be announced:\n{}",
        run.output()
    );
}

/// A command that cannot be STARTED has judged nothing. Reporting that as a
/// lint failure sends someone hunting for an error that does not exist.
#[test]
fn a_missing_command_is_a_gap_not_a_failure() {
    let r = Repo::new();
    manifest(
        &r,
        "pre-commit  shellcheck  *  block  ./no-such-binary-c8f2\n",
    );

    let run = r.hook("pre-commit", &[]);
    assert!(
        run.passed(),
        "an absent tool must not block:\n{}",
        run.output()
    );
    assert!(
        run.says("could not run"),
        "and must be reported as a gap:\n{}",
        run.output()
    );
}

/// The rule the module commits to: a line nobody can parse is a check that is
/// not running, and silence there is the failure `Outcome` exists to name.
#[test]
fn a_malformed_line_is_reported_on_every_commit() {
    let r = Repo::new();
    manifest(&r, "pre-commit  shellcheck  *  LOUD  ./probe.sh\n");

    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "it must not block:\n{}", run.output());
    assert!(run.says("shellcheck"), "{}", run.output());
    assert!(
        run.says("severity"),
        "say what was wrong:\n{}",
        run.output()
    );
    assert!(run.says("line 1"), "and where:\n{}", run.output());
}

/// Externals are appended, never interleaved: a third-party command must not be
/// able to delay `pre-push-branch-protect`.
#[test]
fn a_declared_pre_push_check_runs_after_the_built_ins() {
    let r = Repo::new();
    probe(&r, PROBE, 0);
    manifest(&r, &format!("pre-push  smoke  *  block  ./{PROBE}\n"));
    r.commit("init");
    // A branch name the built-in pattern check accepts, so the only thing that
    // can end the chain early is the one this test is about.
    r.git(&["checkout", "-q", "-b", "feat/x"]);

    let (code, out) = pre_push(&r);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("probe-"), "the declared check must run: {out}");

    // With a branch name the built-in rejects, it blocks first and the declared
    // check never gets a turn — which is the ordering guarantee, observed.
    r.git(&["checkout", "-q", "-b", "nonsense-branch-name"]);
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
        .write_all(b"refs/heads/nonsense aaa refs/heads/nonsense bbb\n")
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(out.status.code(), Some(0), "{text}");
    assert!(
        !text.contains("probe-"),
        "a declared check ran despite a built-in blocking first: {text}"
    );
}

/// At push time nothing is staged, so a pre-push scope evaluated against the
/// index would match nothing and the check would never run. The range being
/// PUSHED is what it has to be judged against.
#[test]
fn a_pre_push_scope_is_judged_against_the_pushed_range() {
    let r = Repo::new();
    probe(&r, PROBE, 0);
    manifest(&r, &format!("pre-push  smoke  *.rs  block  ./{PROBE}\n"));
    r.commit("init");
    r.git(&["checkout", "-q", "-b", "feat/x"]);
    let base = String::from_utf8_lossy(&r.git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();

    r.stage("notes.md", "nothing to compile\n");
    r.commit("docs only");
    let docs = String::from_utf8_lossy(&r.git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();
    let (code, out) = push_range(&r, &base, &docs);
    assert_eq!(code, 0, "{out}");
    assert!(
        !out.contains("probe-"),
        "a docs-only push must not fire a `*.rs` check: {out}"
    );

    r.stage("src/main.rs", "fn main() {}\n");
    r.commit("some rust");
    let rust = String::from_utf8_lossy(&r.git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();
    let (code, out) = push_range(&r, &docs, &rust);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("probe-"),
        "a push containing Rust must fire it: {out}"
    );
}

/// No manifest is the normal case for ninety-six repositories, and it must cost
/// nothing and say nothing.
#[test]
fn a_repository_without_a_manifest_is_unaffected() {
    let r = Repo::new();
    r.stage("a.txt", "hello\n");
    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "{}", run.output());
    assert!(
        !run.says("githooks.conf"),
        "nothing should mention a file that does not exist:\n{}",
        run.output()
    );
}
