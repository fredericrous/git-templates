//! Checks that repair what they find.
//!
//! Safe only because the pre-commit stage holds unstaged changes aside: the
//! tree contains the staged content and nothing else, so anything a formatter
//! touched is by definition part of this commit. Without that, re-staging would
//! sweep in work the author deliberately kept back — which is why this is the
//! last piece of the spec rather than the first.

mod common;
use common::Repo;

/// A stand-in for prettier: `--check` fails on BAD, `--write` repairs it.
///
/// Deliberately not the real thing — the test is about the fix-and-restage
/// machinery, and a real formatter would make it a test of that formatter's
/// version.
fn fake_prettier(r: &Repo) {
    r.write("package.json", "{ \"name\": \"x\" }\n");
    r.write(".prettierrc", "{}\n");
    let bin = r.path("node_modules/.bin");
    std::fs::create_dir_all(&bin).expect("mkdir");
    let script = "#!/bin/sh\nmode=check\nfor a in \"$@\"; do case \"$a\" in --write) mode=write;; esac; done\nrc=0\nfor f in \"$@\"; do case \"$f\" in --*) continue;; esac; [ -f \"$f\" ] || continue\n if grep -q BAD \"$f\"; then if [ \"$mode\" = write ]; then sed -i.bak 's/BAD/GOOD/' \"$f\" && rm -f \"$f.bak\"; else rc=1; fi; fi\ndone\nexit $rc\n";
    let p = bin.join("prettier");
    std::fs::write(&p, script).expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}

/// The default. A hook that edits your files unasked is a larger surprise than
/// one that complains.
#[test]
fn fixing_is_off_unless_asked_for() {
    let r = Repo::new();
    fake_prettier(&r);
    r.stage("a.js", "const a = BAD\n");

    let run = r.hook("pre-commit", &[]);
    assert!(
        !run.passed(),
        "should report, not repair:\n{}",
        run.output()
    );
    assert_eq!(
        std::fs::read_to_string(r.path("a.js")).expect("read"),
        "const a = BAD\n",
        "it edited a file without being asked"
    );
}

#[test]
fn with_fixing_on_it_repairs_and_restages() {
    let r = Repo::new();
    fake_prettier(&r);
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
    fake_prettier(&r);
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
    fake_prettier(&r);
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
