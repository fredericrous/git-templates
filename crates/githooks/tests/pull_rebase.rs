//! pre-push-pull-rebase, ported from its zsh suite.
//!
//! The hardening is the point and is preserved exactly: never touch a dirty
//! tree, rebase only onto the branch's OWN upstream (an older version used
//! `origin HEAD`, which resolves to the remote's default branch and silently
//! rebased every push onto main), and abort cleanly on conflict.

mod common;
use common::Repo;

fn with_origin() -> Repo {
    let r = Repo::new();
    r.stage("a.txt", "x\n");
    r.commit("init");
    // The bare origin goes under .git/, NOT in the working tree. Anywhere in
    // the tree it shows as untracked, the tree is dirty, and every case below
    // silently takes the dirty-tree early exit instead of the path it claims
    // to test. The zsh suite hit this and worked around it with a committed
    // .gitignore; putting the repo somewhere git never scans removes the
    // hazard rather than papering over it.
    let origin = r.path(".git/test-origin.git");
    r.git(&["init", "-q", "--bare", origin.to_str().unwrap()]);
    r.git(&["remote", "add", "origin", origin.to_str().unwrap()]);
    assert!(
        String::from_utf8_lossy(&r.git(&["status", "--porcelain"]).stdout)
            .trim()
            .is_empty(),
        "the fixture itself must leave a CLEAN tree, or these tests prove nothing"
    );
    r
}

#[test]
fn skips_a_branch_with_no_upstream() {
    let r = with_origin();
    r.git(&["checkout", "-q", "-b", "feat/brand-new"]);
    assert!(r.hook("pre-push-pull-rebase", &[]).passed());
}

/// The guard that matters most. The zsh version asserted only that the file
/// survived — which the no-upstream exit satisfies just as well, so deleting
/// the guard entirely still passed. Assert the guard's OWN message instead: it
/// is step 1, so it fires whatever the upstream state.
#[test]
fn announces_the_skip_on_a_dirty_tree() {
    let r = with_origin();
    r.write("scratch.txt", "dirty\n");
    let run = r.hook("pre-push-pull-rebase", &[]);
    assert!(run.passed());
    assert!(run.says("Uncommitted changes"), "the guard did not fire");
    assert!(
        r.path("scratch.txt").exists(),
        "work must not be stashed away"
    );
}

#[test]
fn passes_when_in_sync_with_its_own_upstream() {
    let r = with_origin();
    r.git(&["checkout", "-q", "-b", "feat/synced"]);
    r.git(&["push", "-q", "--no-verify", "-u", "origin", "feat/synced"]);
    let run = r.hook("pre-push-pull-rebase", &[]);
    assert!(run.passed());
    assert!(run.says("in sync"));
}

/// The state a local rebase or amend leaves behind, and the reason this copy
/// was rewritten: the hook used to prescribe `git pull --rebase` as THE fix,
/// which after a rebase replays the upstream commits you just rewrote — the one
/// command that undoes the work you are pushing.
#[test]
fn divergence_offers_both_readings_and_prescribes_neither() {
    let r = with_origin();
    r.git(&["checkout", "-q", "-b", "feat/diverged"]);
    r.git(&["push", "-q", "--no-verify", "-u", "origin", "feat/diverged"]);

    // One commit pushed, then rewritten locally: 1 ahead, 1 behind.
    r.stage("a.txt", "one\n");
    r.commit("first");
    r.git(&["push", "-q", "--no-verify", "origin", "feat/diverged"]);
    r.git(&["reset", "-q", "--hard", "HEAD~1"]);
    r.stage("a.txt", "one, rewritten\n");
    r.commit("first, amended");

    let run = r.hook("pre-push-pull-rebase", &[]);
    assert!(
        run.passed(),
        "divergence never blocked a push:\n{}",
        run.output()
    );
    assert!(run.says("diverged"), "{}", run.output());
    // How far apart, which the predicate used to compute and discard.
    assert!(
        run.says("1 ahead, 1 behind"),
        "the counts must be in the message:\n{}",
        run.output()
    );
    // Both readings offered, so neither is prescribed.
    assert!(
        run.says("--force-with-lease"),
        "the rebase reading is missing:\n{}",
        run.output()
    );
    assert!(
        run.says("pull --rebase"),
        "the someone-else-pushed reading is missing:\n{}",
        run.output()
    );
}

/// The normal state right after a PR squash-merges with delete-on-merge: the
/// upstream is configured locally but gone on the remote. `git pull --rebase`
/// would fail on the missing ref and read as a conflict, wrongly blocking.
#[test]
fn skips_when_the_upstream_was_deleted_on_the_remote() {
    let r = with_origin();
    r.git(&["checkout", "-q", "-b", "feat/merged-away"]);
    r.git(&[
        "push",
        "-q",
        "--no-verify",
        "-u",
        "origin",
        "feat/merged-away",
    ]);
    // Delete the branch INSIDE the bare repo, not via `push --delete`: the
    // latter also prunes the local remote-tracking ref, so `@{u}` stops
    // resolving and the hook exits at the no-upstream step instead of reaching
    // the "upstream vanished" one this case is about. The zsh suite did it this
    // way for the same reason.
    let origin = r.path(".git/test-origin.git");
    std::process::Command::new("git")
        .args([
            "-C",
            origin.to_str().unwrap(),
            "update-ref",
            "-d",
            "refs/heads/feat/merged-away",
        ])
        .status()
        .expect("delete the remote branch");
    let run = r.hook("pre-push-pull-rebase", &[]);
    assert!(run.passed());
    assert!(run.says("no longer exists"));
}

/// The incident this guard exists for: `git worktree add -b <new> <path>
/// main` sets the new branch's upstream to the LOCAL branch `main` — no `/`
/// in `@{u}` at all. The old code split on `/` and fell back to `("origin",
/// upstream)`, so it silently treated "main" as "origin/main" and ran a bare
/// `pull --rebase`, which actually synced from LOCAL main per
/// `branch.*.remote`/`.merge` — not origin, whatever the messages said. If
/// local main is later moved (a `reset --hard` in another worktree, say),
/// the next push rebases onto wherever it ended up, unannounced. The fix
/// does not merely hope a same-history rebase is harmless here — it never
/// attempts one at all when the upstream is not a real remote.
#[test]
fn a_branch_tracking_a_local_branch_is_not_silently_synced_as_origin() {
    let r = with_origin();
    r.git(&["checkout", "-q", "-b", "feat/from-local"]);
    r.stage("b.txt", "extra work\n");
    r.commit("extra work");
    // No `-u origin`: track LOCAL main instead, exactly what
    // `git worktree add -b <new> <path> main` does by default.
    r.git(&["branch", "--set-upstream-to=main", "feat/from-local"]);
    let before = String::from_utf8_lossy(&r.git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();

    let run = r.hook("pre-push-pull-rebase", &[]);
    assert!(run.passed(), "{}", run.output());
    assert!(
        run.says("not a remote-tracking branch"),
        "a local-branch upstream must be named and skipped, not guessed at as origin: {}",
        run.output()
    );

    let after = String::from_utf8_lossy(&r.git(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();
    assert_eq!(
        before, after,
        "the branch must not be touched at all when its upstream is not a real remote"
    );
}
