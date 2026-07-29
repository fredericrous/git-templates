//! The pre-commit and pre-push dispatchers.
//!
//! They are NOT the same shape and both shapes are load-bearing — pre-commit
//! runs sub-hooks in parallel and reports EVERY failure; pre-push runs them
//! serially and stops at the FIRST. A shared "run all" helper is the obvious
//! way to lose that distinction, so both are pinned here.

mod common;
use common::Repo;

#[test]
fn pre_commit_exits_zero_with_no_sub_hooks() {
    assert!(Repo::new().hook("pre-commit", &[]).passed());
}

#[test]
fn pre_commit_runs_every_sub_hook() {
    let r = Repo::new();
    for n in ["a", "b", "c"] {
        r.sub_hook(&format!("pre-commit-{n}"), &format!("echo {n} >> \"$PWD/ran.txt\"\n"));
    }
    assert!(r.hook("pre-commit", &[]).passed());
    let mut got: Vec<String> = std::fs::read_to_string(r.path("ran.txt"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    got.sort();
    assert_eq!(got, vec!["a", "b", "c"]);
}

/// A rendezvous, not a stopwatch: each sub-hook announces itself then waits for
/// all three. Under real parallelism they release each other immediately; run
/// serially the first can never see the others and times out. Independent of
/// how fast the machine is — an earlier wall-clock version of this was flaky.
#[test]
fn pre_commit_runs_sub_hooks_in_parallel() {
    let r = Repo::new();
    let body = r#"echo here >> "$PWD/rv.txt"
i=0
while [ "$(wc -l < "$PWD/rv.txt")" -lt 3 ] && [ $i -lt 50 ]; do sleep 0.1; i=$((i+1)); done
[ "$(wc -l < "$PWD/rv.txt")" -ge 3 ]
"#;
    for n in ["a", "b", "c"] {
        r.sub_hook(&format!("pre-commit-{n}"), body);
    }
    assert!(
        r.hook("pre-commit", &[]).passed(),
        "sub-hooks never met — they ran serially"
    );
}

/// Fixing one lint error, committing, and immediately meeting the next is the
/// behaviour this prevents.
#[test]
fn pre_commit_reports_every_failure_not_just_the_first() {
    let r = Repo::new();
    r.sub_hook("pre-commit-alpha", "exit 1\n");
    r.sub_hook("pre-commit-beta", "exit 0\n");
    r.sub_hook("pre-commit-gamma", "exit 1\n");
    let run = r.hook("pre-commit", &[]);
    assert!(!run.passed());
    assert!(run.says("pre-commit-alpha"));
    assert!(run.says("pre-commit-gamma"));
    assert!(!run.says("pre-commit-beta"), "a passing hook must not be named");
}

/// Substring, not equality — that is what makes
/// `git -c hook.skip=package-lock commit` work.
#[test]
fn hook_skip_is_a_substring_match() {
    let r = Repo::new();
    r.sub_hook("pre-commit-kept", "echo kept >> \"$PWD/s.txt\"\n");
    r.sub_hook("pre-commit-package-lock", "echo gone >> \"$PWD/s.txt\"\n");
    r.git(&["config", "--add", "hook.skip", "package-lock"]);
    assert!(r.hook("pre-commit", &[]).passed());
    assert_eq!(std::fs::read_to_string(r.path("s.txt")).unwrap().trim(), "kept");
}

#[test]
fn pre_commit_skips_everything_during_a_cherry_pick() {
    let r = Repo::new();
    r.sub_hook("pre-commit-fails", "exit 1\n");
    assert!(!r.hook("pre-commit", &[]).passed());
    std::fs::write(r.path(".git/CHERRY_PICK_HEAD"), "x").unwrap();
    assert!(r.hook("pre-commit", &[]).passed());
}

#[test]
fn arguments_reach_sub_hooks() {
    let r = Repo::new();
    r.sub_hook("pre-commit-echo", "echo \"$@\" > \"$PWD/args.txt\"\n");
    assert!(r.hook("pre-commit", &["one", "two"]).passed());
    assert_eq!(
        std::fs::read_to_string(r.path("args.txt")).unwrap().trim(),
        "one two"
    );
}

#[test]
fn pre_push_runs_sub_hooks_in_glob_order() {
    let r = Repo::new();
    for n in ["aaa", "mmm", "zzz"] {
        r.sub_hook(&format!("pre-push-{n}"), &format!("echo {n} >> \"$PWD/o.txt\"\n"));
    }
    assert!(r.hook("pre-push", &[]).passed());
    assert_eq!(
        std::fs::read_to_string(r.path("o.txt")).unwrap().replace('\n', ""),
        "aaammmzzz"
    );
}

/// The difference from pre-commit: once one step fails the rest MUST NOT run.
/// `zzz` stands for the expensive test suite whose preconditions are gone.
#[test]
fn pre_push_stops_at_the_first_failure() {
    let r = Repo::new();
    r.sub_hook("pre-push-aaa", "echo aaa >> \"$PWD/st.txt\"\n");
    r.sub_hook("pre-push-mmm", "echo mmm >> \"$PWD/st.txt\"\nexit 3\n");
    r.sub_hook("pre-push-zzz", "echo zzz >> \"$PWD/st.txt\"\n");
    let run = r.hook("pre-push", &[]);
    assert_eq!(run.code, 3, "the sub-hook's own code must propagate");
    assert_eq!(
        std::fs::read_to_string(r.path("st.txt")).unwrap().replace('\n', ""),
        "aaammm",
        "zzz should not have run"
    );
    // singular message, distinct from pre-commit's list
    assert!(run.says("Error raised by hook"));
    assert!(run.says("pre-push-mmm"));
}
