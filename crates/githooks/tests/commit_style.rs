//! The commit-style settings as a user meets them: `githooks list` and
//! `githooks setup`.
//!
//! The wizard's question flow is unit-tested in `setup.rs` against a `BufRead`,
//! which is what lets it be driven without a pseudo-terminal. What is tested
//! here is the part that only exists once there is a real process: the verb, its
//! arguments, and the answer given when nobody is at the keyboard.

mod common;
use common::Repo;

#[test]
fn list_reports_the_settings_in_effect() {
    let r = Repo::new();
    let out = r.run(&["list"]).output();
    assert!(out.contains("commit style"), "{out}");
    for row in ["gitmoji", "subject max", "description max", "body wrap"] {
        assert!(out.contains(row), "{row} missing from: {out}");
    }
}

/// The block is printed even when nothing has been configured — the defaults
/// are the divisive part, and somebody who wants them changed has no reason to
/// guess that four keys exist.
#[test]
fn the_defaults_are_shown_and_so_is_the_way_to_change_them() {
    let r = Repo::new();
    let out = r.run(&["list"]).output();
    assert!(out.contains("none"), "the default placement: {out}");
    assert!(out.contains("72"), "the default subject limit: {out}");
    assert!(out.contains("githooks setup"), "no pointer at all: {out}");
}

/// A configured value shows what set it and where it came from, the same
/// declared-vs-effective reporting `githooks list` already does for severity.
#[test]
fn a_configured_value_names_its_key_and_scope() {
    let r = Repo::new();
    r.git(&["config", "githooks.commit.gitmoji", "suffix"]);
    let out = r.run(&["list"]).output();
    assert!(out.contains("suffix"), "{out}");
    assert!(out.contains("githooks.commit.gitmoji"), "{out}");
    assert!(out.contains("local"), "the scope was not reported: {out}");
}

/// `--stage` narrows the CHECKS. Commit style belongs to no stage, so
/// suppressing it there would only hide it from the reader who narrowed.
#[test]
fn narrowing_to_one_stage_still_shows_the_commit_style() {
    let r = Repo::new();
    let out = r.run(&["list", "--stage", "pre-push"]).output();
    assert!(out.contains("commit style"), "{out}");
}

#[test]
fn the_json_carries_the_same_answer() {
    let r = Repo::new();
    r.git(&["config", "githooks.commit.subjectMax", "100"]);
    let out = r.run(&["list", "--json"]).stdout;
    assert!(out.contains("\"commit_style\""), "{out}");
    // Numbers as numbers: a limit a reader will compare against should not
    // have to be parsed back out of a string.
    assert!(out.contains("\"value\":100"), "{out}");
    assert!(out.contains("\"default\":72"), "{out}");
    assert!(out.contains("\"overridden\":true"), "{out}");
}

/// A setting that cannot do what it looks like it does is reported by the
/// commands whose job is reading configuration back — and by nothing on the
/// commit path, where a coherence essay in front of every commit is how people
/// learn to stop reading hook output.
#[test]
fn an_incoherent_pair_is_reported_by_list_and_not_by_the_hook() {
    let r = Repo::new();
    r.git(&["config", "githooks.commit.subjectMax", "52"]);
    r.git(&["config", "githooks.commit.descriptionMax", "50"]);

    let listed = r.run(&["list"]).output();
    assert!(listed.contains("can never bind"), "{listed}");

    r.write("MSG", "feat: a short one\n");
    let hooked = r.hook("commit-msg", &["MSG"]).output();
    assert!(
        !hooked.contains("can never bind"),
        "the hook should not lecture: {hooked}"
    );
}

/// Not a terminal: say so on stderr, print the commands on stdout, exit 0.
///
/// `githooks setup > setup.sh` in a provisioning script should produce
/// something usable rather than an error.
#[test]
fn setup_without_a_terminal_prints_the_commands_and_succeeds() {
    let r = Repo::new();
    let run = r.run(&["setup"]);
    assert!(run.passed(), "exit {}: {}", run.code, run.output());
    assert!(run.stderr.contains("not a terminal"), "{}", run.stderr);
    for key in [
        "githooks.commit.gitmoji",
        "githooks.commit.subjectMax",
        "githooks.commit.descriptionMax",
        "githooks.commit.bodyWrap",
    ] {
        assert!(run.stdout.contains(key), "{key} missing: {}", run.stdout);
    }
    // The refusal must not land in the redirected file.
    assert!(!run.stdout.contains("not a terminal"), "{}", run.stdout);
}

/// What it prints is what is in effect, not what shipped.
#[test]
fn the_printed_commands_carry_the_current_values() {
    let r = Repo::new();
    r.git(&["config", "githooks.commit.gitmoji", "replace"]);
    let out = r.run(&["setup"]).stdout;
    assert!(out.contains("githooks.commit.gitmoji replace"), "{out}");
}

#[test]
fn setup_refuses_contradictory_scopes() {
    let r = Repo::new();
    let run = r.run(&["setup", "--local", "--global"]);
    assert!(!run.passed());
    assert!(run.output().contains("contradict"), "{}", run.output());
}

#[test]
fn setup_rejects_an_argument_it_does_not_know() {
    let r = Repo::new();
    let run = r.run(&["setup", "--wat"]);
    assert!(!run.passed());
    assert!(run.output().contains("--wat"), "{}", run.output());
    assert!(run.output().contains("usage"), "{}", run.output());
}

/// `setup` is a verb like any other, and `--help` must name it or nobody finds
/// the thing this whole feature exists to make findable.
#[test]
fn help_names_the_new_verb() {
    let r = Repo::new();
    let out = r.run(&["--help"]).output();
    assert!(out.contains("githooks setup"), "{out}");
}

// The older boolean keys (`githooks.fix`, `githooks.testPushedTree`) moved onto
// the same reader in this series. They are asserted where they are actually
// read — see `gits_own_spelling_of_true_turns_it_on` in `pushed_tree.rs` — since
// a key nothing consults in a given run cannot prove anything here.
