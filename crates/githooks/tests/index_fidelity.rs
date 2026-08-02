//! The checks must judge what is being committed, not what happens to be on
//! disk.
//!
//! The dangerous half is the restore. A patch taken and not put back loses
//! uncommitted work, which is worse than either failure that overwrote tracked
//! files, because there is nothing on disk to recover from. Most of these tests
//! are about that.

mod common;
use common::Repo;
use std::process::Command;

const VALID: &str = "{\n  \"a\": 2\n}\n";
const BROKEN: &str = "{ THIS IS NOT JSON\n";

fn seed(r: &Repo) {
    r.stage("x.json", "{\n  \"a\": 1\n}\n");
    r.commit("chore: seed");
}

fn tree(r: &Repo, path: &str) -> String {
    std::fs::read_to_string(r.path(path)).expect("read")
}

/// The false POSITIVE: staged content is valid, the tree is not, and the hook
/// blocked a commit that was about to be correct.
#[test]
fn a_check_judges_the_staged_content_not_the_tree() {
    let r = Repo::new();
    seed(&r);
    r.stage("x.json", VALID);
    r.write("x.json", BROKEN);

    let run = r.hook("pre-commit", &[]);
    assert!(
        run.passed(),
        "judged the working tree, not the commit:\n{}",
        run.output()
    );
    // …and the tree is exactly as the author left it.
    assert_eq!(tree(&r, "x.json"), BROKEN, "unstaged work was not restored");
}

/// The false NEGATIVE, which is the worse direction: the tree is fine and the
/// staged content is not, so a broken commit sails through.
#[test]
fn a_broken_staged_change_is_still_caught() {
    let r = Repo::new();
    seed(&r);
    r.stage("x.json", BROKEN);
    r.write("x.json", VALID);

    let run = r.hook("pre-commit", &[]);
    assert!(
        !run.passed(),
        "a broken staged change passed because the tree was fine:\n{}",
        run.output()
    );
    assert_eq!(tree(&r, "x.json"), VALID, "unstaged work was not restored");
}

/// Git QUOTES non-ASCII bytes in `--name-only` output by default — `é.json`
/// prints as the literal text `"\303\251.json"`. A line-oriented reader that
/// treated that as a real path would never find the actual file, record a
/// false "absent" marker for it, and restore nothing where the real unstaged
/// bytes belong — silent data loss on any filename outside ASCII.
#[test]
fn a_non_ascii_path_survives_the_stash_and_restore() {
    let r = Repo::new();
    r.stage("é.json", "{\n  \"a\": 1\n}\n");
    r.commit("chore: seed");
    r.stage("é.json", VALID);
    r.write("é.json", BROKEN);

    let run = r.hook("pre-commit", &[]);
    assert!(
        run.passed(),
        "judged the working tree, not the commit:\n{}",
        run.output()
    );
    assert_eq!(
        tree(&r, "é.json"),
        BROKEN,
        "unstaged work on a non-ASCII path was not restored"
    );
}

/// The common case, and it must cost nothing: a clean tree is never touched.
#[test]
fn nothing_unstaged_means_nothing_is_moved() {
    let r = Repo::new();
    seed(&r);
    r.stage("x.json", VALID);

    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "{}", run.output());
    assert_eq!(tree(&r, "x.json"), VALID);
    assert!(
        !r.path(".git/githooks-held").exists(),
        "held something with nothing to hold"
    );
}

/// A failing check must not cost the author their unstaged work.
#[test]
fn a_blocked_commit_still_restores() {
    let r = Repo::new();
    seed(&r);
    r.stage("x.json", BROKEN);
    r.write("bad.txt", "unstaged and precious\n");
    // Make the working tree differ so a patch is actually taken.
    r.write("x.json", "{\n  \"a\": 9\n}\n");

    let run = r.hook("pre-commit", &[]);
    assert!(!run.passed(), "guard: the staged JSON is broken");
    assert_eq!(
        tree(&r, "x.json"),
        "{\n  \"a\": 9\n}\n",
        "a blocked commit lost the unstaged change"
    );
    assert!(
        !r.path(".git/githooks-held").exists(),
        "held files left behind after a block"
    );
}

/// Untracked files are not part of this commit and must not be moved — losing
/// somebody's new file would be the same failure by another route.
#[test]
fn untracked_files_are_left_alone() {
    let r = Repo::new();
    seed(&r);
    r.stage("x.json", VALID);
    r.write("x.json", BROKEN);
    std::fs::write(r.path("scratch.txt"), "not staged, not tracked\n").expect("write");

    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "{}", run.output());
    assert_eq!(
        std::fs::read_to_string(r.path("scratch.txt")).expect("read"),
        "not staged, not tracked\n",
        "an untracked file was moved"
    );
}

/// Mid-merge the tree already holds somebody else's work, so taking a patch of
/// it would be the wrong instrument entirely.
#[test]
fn nothing_is_stashed_mid_merge() {
    let r = Repo::new();
    seed(&r);
    r.stage("x.json", VALID);
    r.write("x.json", BROKEN);
    std::fs::write(r.path(".git/MERGE_HEAD"), "deadbeef\n").expect("write");

    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "{}", run.output());
    assert_eq!(tree(&r, "x.json"), BROKEN, "the tree was touched mid-merge");
    assert!(!r.path(".git/githooks-held").exists());
}

/// Trust a declared manifest non-interactively, the way `external.rs` does —
/// `githooks trust` performs the decision outright when run directly, unlike
/// `install`'s offer, which needs a confirming tty.
#[cfg(unix)]
fn trust(r: &Repo) {
    let out = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("trust")
        .current_dir(&r.dir)
        .output()
        .expect("githooks trust");
    assert!(out.status.success(), "could not trust the manifest");
}

/// Ctrl-C DURING pre-commit, not after: this is the exact window the signal
/// handler exists for, and the one a handler that itself runs `git`,
/// allocates and prints risks deadlocking in — if the interrupted thread
/// already held the allocator's or stdio's lock, non-async-signal-safe code
/// running IN the handler would wait on it forever.
///
/// A declared `sleep 2` check is the deterministic way to guarantee the child
/// is still mid-run when the signal arrives: it opens a wide, fixed window
/// (send SIGINT any time in the first ~1.8s) rather than racing a real check's
/// unpredictable, sub-millisecond duration.
#[cfg(unix)]
#[test]
fn ctrl_c_mid_run_still_restores_and_dies_by_the_signal() {
    use std::os::unix::process::ExitStatusExt;

    let r = Repo::new();
    r.stage(".githooks.conf", "pre-commit  slowpoke  *  warn  sleep 2\n");
    trust(&r);
    r.stage("x.json", VALID);
    r.commit("chore: seed");
    r.stage("x.json", VALID);
    r.write("x.json", BROKEN);

    let mut child = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("--hooks-dir")
        .arg(r.path(".git/hooks"))
        .arg("pre-commit")
        .current_dir(&r.dir)
        .spawn()
        .expect("spawn githooks pre-commit");

    // Comfortably inside the 2s window the declared check guarantees, and
    // comfortably past the moment `StagedOnly::enter` has already run —
    // parking the unstaged bytes and installing the handler happens before
    // any check, declared or built-in, starts.
    std::thread::sleep(std::time::Duration::from_millis(300));

    let status_kill = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("send SIGINT");
    assert!(status_kill.success(), "could not signal the child");

    let status = child.wait().expect("wait on the signalled child");
    assert_eq!(
        status.signal(),
        Some(2),
        "must die BY SIGINT (re-raised with the default handler), not exit some other way: {status:?}"
    );
    assert_eq!(
        tree(&r, "x.json"),
        BROKEN,
        "unstaged work was not restored after Ctrl-C"
    );
    assert!(
        !r.path(".git/githooks-held").exists(),
        "held files left behind after the signal-handler restore"
    );
}

/// `githooks restore` is the recovery path for when even the signal handler was
/// interrupted.
#[test]
fn restore_puts_back_held_files() {
    let r = Repo::new();
    seed(&r);
    r.stage("x.json", VALID);
    r.write("x.json", BROKEN);

    // Park the file the way the guard does, then abandon it.
    std::fs::create_dir_all(r.path(".git/githooks-held")).expect("mkdir");
    std::fs::write(r.path(".git/githooks-held/x.json"), BROKEN).expect("write");
    Command::new("git")
        .arg("-C")
        .arg(&r.dir)
        .args(["checkout", "--", "."])
        .output()
        .expect("git checkout");
    assert_eq!(
        tree(&r, "x.json"),
        VALID,
        "fixture: tree reset to the index"
    );

    let out = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("restore")
        .current_dir(&r.dir)
        .output()
        .expect("githooks restore");
    assert!(out.status.success(), "{:?}", out);
    assert_eq!(tree(&r, "x.json"), BROKEN, "restore did not put it back");
    assert!(!r.path(".git/githooks-held").exists());
}

/// Nothing to restore is not an error — it is how somebody checks.
#[test]
fn restore_with_nothing_parked_is_fine() {
    let r = Repo::new();
    seed(&r);
    let out = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("restore")
        .current_dir(&r.dir)
        .output()
        .expect("githooks restore");
    assert!(out.status.success());
}
