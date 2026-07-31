//! Checks that repair what they find.
//!
//! Safe only because the pre-commit stage holds unstaged changes aside: the
//! tree contains the staged content and nothing else, so anything a formatter
//! touched is by definition part of this commit. Without that, re-staging would
//! sweep in work the author deliberately kept back — which is why this is the
//! last piece of the spec rather than the first.

mod common;
use common::Repo;

/// A rewriting probe, in whatever form this platform can execute.
///
/// The same shape `tests/external.rs` uses, because it is the one fixture
/// proven to run on all three platforms — a `#!/bin/sh` file has no
/// interpreter on Windows, which is exactly how the first version of this test
/// failed there.
#[cfg(unix)]
fn rewriter(r: &Repo, name: &str) {
    r.write(
        name,
        "#!/bin/sh\nfor f in *.js; do [ -f \"$f\" ] || continue; sed -i.bak 's/BAD/GOOD/' \"$f\" && rm -f \"$f.bak\"; done\n",
    );
    let p = r.path(name);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    r.git(&["add", name]);
}

#[cfg(not(unix))]
fn rewriter(r: &Repo, name: &str) {
    r.write(
        name,
        "@echo off\r\nfor %%f in (*.js) do powershell -NoProfile -Command \"(Get-Content '%%f') -replace 'BAD','GOOD' | Set-Content -NoNewline '%%f'\"\r\n",
    );
    r.git(&["add", name]);
}

#[cfg(unix)]
const REWRITER: &str = "rewrite.sh";
#[cfg(not(unix))]
const REWRITER: &str = "rewrite.cmd";

/// Declare it as an external so the test exercises the same machinery a user
/// would reach for, on every platform.
fn manifest_with_fixer(r: &Repo) {
    rewriter(r, REWRITER);
    r.stage(
        ".githooks.conf",
        &format!("pre-commit  fmt  *.js  block  fix ./{REWRITER}\n"),
    );
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("trust")
        .current_dir(&r.dir)
        .output()
        .expect("githooks trust");
    assert!(out.status.success(), "could not trust the manifest");
}

/// The default. A hook that edits your files unasked is a larger surprise than
/// one that complains.
#[test]
fn fixing_is_off_unless_asked_for() {
    let r = Repo::new();
    manifest_with_fixer(&r);
    r.stage("a.js", "const a = BAD\n");

    let run = r.hook("pre-commit", &[]);
    // The declared command is not even invoked: fixing is all it does, and
    // nobody asked for a fix.
    assert_eq!(
        std::fs::read_to_string(r.path("a.js")).expect("read"),
        "const a = BAD\n",
        "it edited a file without being asked"
    );
    assert!(!run.says("fixed and re-staged"), "{}", run.output());
}

#[test]
fn with_fixing_on_it_repairs_and_restages() {
    let r = Repo::new();
    manifest_with_fixer(&r);
    r.git(&["config", "githooks.fix", "true"]);
    r.stage("a.js", "const a = BAD\n");

    let run = r.hook("pre-commit", &[]);
    assert!(
        run.passed(),
        "the repair should let the commit through:\n{}",
        run.output()
    );
    assert_eq!(
        std::fs::read_to_string(r.path("a.js")).expect("read"),
        "const a = GOOD\n"
    );
    // Staged, not merely written — otherwise the commit would not contain it.
    let staged = r.git(&["show", ":a.js"]);
    assert_eq!(String::from_utf8_lossy(&staged.stdout), "const a = GOOD\n");
    // And said out loud: the author's files changed under them.
    assert!(run.says("fixed and re-staged"), "{}", run.output());
}

/// Nothing to repair must not report a repair.
#[test]
fn a_clean_file_is_not_reported_as_fixed() {
    let r = Repo::new();
    manifest_with_fixer(&r);
    r.git(&["config", "githooks.fix", "true"]);
    r.stage("a.js", "const a = GOOD\n");

    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "{}", run.output());
    assert!(!run.says("fixed and re-staged"), "{}", run.output());
}

/// The interaction that makes this safe: a repair must stage only what it
/// touched, never the unstaged work the author kept back.
#[test]
fn a_repair_does_not_sweep_in_unstaged_work() {
    let r = Repo::new();
    manifest_with_fixer(&r);
    r.git(&["config", "githooks.fix", "true"]);
    r.stage("other.txt", "committed\n");
    r.commit("chore: seed");

    r.stage("a.js", "const a = BAD\n");
    // Deliberately kept back.
    r.write("other.txt", "work in progress, not staged\n");

    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "{}", run.output());

    let staged = r.git(&["diff", "--cached", "--name-only"]);
    let staged = String::from_utf8_lossy(&staged.stdout);
    assert!(
        staged.contains("a.js"),
        "the repair was not staged: {staged}"
    );
    assert!(
        !staged.contains("other.txt"),
        "the repair swept in unstaged work: {staged}"
    );
    assert_eq!(
        std::fs::read_to_string(r.path("other.txt")).expect("read"),
        "work in progress, not staged\n",
        "and the unstaged work must still be there"
    );
}
