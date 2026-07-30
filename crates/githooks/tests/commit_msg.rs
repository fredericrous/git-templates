//! commit-msg, ported from its zsh suite.

mod common;
use common::Repo;

/// Run the hook over a message file and return what it left behind.
fn rewrite(r: &Repo, msg: &str) -> (bool, String) {
    r.write("MSG", msg);
    let run = r.hook("commit-msg", &["MSG"]);
    (
        run.passed(),
        std::fs::read_to_string(r.path("MSG")).unwrap_or_default(),
    )
}

#[test]
fn rejects_a_subject_over_72_characters() {
    let r = Repo::new();
    let long = format!("feat: {}", "x".repeat(80));
    assert!(!rewrite(&r, &long).0);
}

#[test]
fn accepts_a_short_conventional_subject() {
    let r = Repo::new();
    assert!(rewrite(&r, "feat: a short one\n").0);
}

#[test]
fn rejects_a_description_over_50_characters() {
    let r = Repo::new();
    let msg = format!("feat: {}", "d".repeat(60));
    assert!(!rewrite(&r, &msg).0);
}

/// The whole subject may run to 72 even when the scope is long, as long as the
/// description after the colon stays under 50.
#[test]
fn a_long_scope_is_fine_while_the_description_is_short() {
    let r = Repo::new();
    assert!(rewrite(&r, "feat(a-very-long-scope-name-here): short desc\n").0);
}

#[test]
fn rejects_a_message_with_no_type_prefix() {
    let r = Repo::new();
    assert!(!rewrite(&r, "just a message\n").0);
}

#[test]
fn rejects_a_prefix_with_no_description() {
    let r = Repo::new();
    assert!(!rewrite(&r, "feat:\n").0);
}

#[test]
fn prepends_the_gitmoji_for_the_type() {
    let r = Repo::new();
    let (ok, out) = rewrite(&r, "feat: add a thing\n");
    assert!(ok);
    assert!(out.starts_with('✨'), "got: {out}");
    assert!(out.contains("feat: add a thing"));
}

#[test]
fn accepts_a_subject_that_already_carries_an_emoji() {
    let r = Repo::new();
    assert!(rewrite(&r, "♻️  refactor: already prefixed\n").0);
}

#[test]
fn accepts_scopes_including_hyphenated_ones() {
    let r = Repo::new();
    assert!(rewrite(&r, "fix(parser): trim\n").0);
    assert!(rewrite(&r, "fix(my-scope): trim\n").0);
}

/// A body quoting a conventional commit — a revert citing what it undid — must
/// not be promoted into the subject. The JS once used /ms flags and rewrote the
/// commit with that body line as the new summary.
#[test]
fn a_body_quoting_a_conventional_commit_is_not_promoted() {
    let r = Repo::new();
    let (ok, out) = rewrite(
        &r,
        "revert: undo it\n\nIn abc1234 (\"fix(scope): thing\")\n",
    );
    assert!(ok);
    assert!(out.lines().next().unwrap().contains("revert: undo it"));
}

#[test]
fn groups_footers_after_one_blank_line() {
    let r = Repo::new();
    let (ok, out) = rewrite(&r, "feat: x\n\nbody\n\nCo-Authored-By: a <a@x>\n");
    assert!(ok);
    assert!(out.ends_with("Co-Authored-By: a <a@x>\n"), "got: {out:?}");
    assert!(out.contains("body\n\nCo-Authored-By"));
}

/// Every type in the vocabulary must be accepted — this is what caught the
/// commit-type/branch-prefix divergence being real rather than theoretical.
#[test]
fn every_declared_commit_type_is_accepted() {
    let r = Repo::new();
    for t in [
        "build", "chore", "docs", "feat", "fix", "perf", "refactor", "revert", "style", "test",
        "add", "remove",
    ] {
        assert!(rewrite(&r, &format!("{t}: a description\n")).0, "{t}");
    }
}
