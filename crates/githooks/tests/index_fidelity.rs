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

/// SIGINT mid-check must still restore. The handler itself only writes a byte
/// to a pipe — a plain thread outside signal context does the actual restore
/// — so this is the one test proving that indirection still ends where the
/// old, simpler, signal-unsafe handler did: the tree back the way the author
/// left it, and the process dead by the signal that hit it.
///
/// `sleep 5` is the declared check: slow enough that the signal is guaranteed
/// to land while it is still the thing running, cheap enough not to make a
/// hung path (a regression back to restoring inline) drag the suite out
/// waiting on it — `recv_timeout` below bounds that instead.
#[cfg(unix)]
#[test]
fn sigint_mid_check_still_restores() {
    let r = Repo::new();
    seed(&r);
    r.stage("x.json", VALID);
    r.write("x.json", BROKEN);
    r.stage(".githooks.conf", "pre-commit  slow  *  block  sleep  5\n");
    let trusted = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("trust")
        .current_dir(&r.dir)
        .output()
        .expect("githooks trust");
    assert!(trusted.status.success(), "could not trust the manifest");

    let mut child = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("--hooks-dir")
        .arg(r.path(".git/hooks"))
        .arg("pre-commit")
        .current_dir(&r.dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn githooks pre-commit");

    // `enter()` parks before any check starts, so this appears almost
    // immediately — long before `sleep 5` could finish on its own.
    let store = r.path(".git/githooks-held");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !store.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "fixture: nothing was ever parked to restore"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let signalled = Command::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()
        .expect("run kill");
    assert!(signalled.success(), "fixture: could not signal the child");

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait());
    });
    let status = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect(
            "githooks did not exit within 10s of SIGINT — \
             the pipe/thread handoff deadlocked",
        )
        .expect("wait on the child");
    assert!(!status.success(), "SIGINT must not look like a clean pass");

    assert_eq!(
        tree(&r, "x.json"),
        BROKEN,
        "unstaged work was not restored after SIGINT"
    );
    assert!(!store.exists(), "held files left behind after SIGINT");
}

/// A symlink is a link, not a file with those bytes in it. `fs::read` follows
/// it — copying the TARGET's content as the "backup" — so a naive park-and-
/// restore quietly turns the link into a plain file. `link` must still be a
/// link afterward, pointing where the author unstaged-left it.
#[test]
fn an_unstaged_symlink_retarget_survives_the_hook() {
    let r = Repo::new();
    seed(&r);
    if !try_symlink("x.json", &r.path("link")) {
        println!("  ! symlinks unavailable — skipping");
        return;
    }
    r.git(&["add", "link"]);
    r.commit("chore: add a symlink");

    // Unstaged: repoint the tracked link elsewhere.
    std::fs::remove_file(r.path("link")).expect("remove the old link");
    assert!(
        try_symlink("unstaged-target", &r.path("link")),
        "fixture: retarget failed after the capability check passed"
    );

    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "{}", run.output());

    let meta = std::fs::symlink_metadata(r.path("link")).expect("link must still exist");
    assert!(
        meta.file_type().is_symlink(),
        "the symlink was replaced with a regular file"
    );
    assert_eq!(
        std::fs::read_link(r.path("link")).expect("read link"),
        std::path::Path::new("unstaged-target"),
        "the unstaged retarget was not restored"
    );
}

/// A symlink mid-retarget commonly points at nothing for a moment. `fs::read`
/// on a dangling link fails exactly like it does on a deleted file, so this
/// must not be mistaken for one — that reads the working tree, decides the
/// link was deleted, and deletes it again on restore.
#[test]
fn a_dangling_unstaged_symlink_is_not_deleted() {
    let r = Repo::new();
    seed(&r);
    if !try_symlink("x.json", &r.path("link")) {
        println!("  ! symlinks unavailable — skipping");
        return;
    }
    r.git(&["add", "link"]);
    r.commit("chore: add a symlink");

    std::fs::remove_file(r.path("link")).expect("remove the old link");
    assert!(
        try_symlink("does-not-exist", &r.path("link")),
        "fixture: retarget failed after the capability check passed"
    );

    let run = r.hook("pre-commit", &[]);
    assert!(run.passed(), "{}", run.output());

    let meta = std::fs::symlink_metadata(r.path("link"))
        .expect("a dangling symlink was deleted instead of restored");
    assert!(meta.file_type().is_symlink());
    assert_eq!(
        std::fs::read_link(r.path("link")).expect("read link"),
        std::path::Path::new("does-not-exist"),
    );
}

/// `None` on a platform/runner that cannot create the link at all (Windows
/// without `SeCreateSymbolicLinkPrivilege`), so the two tests above skip
/// instead of failing on a capability the environment never had.
fn try_symlink(target: &str, link: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        false
    }
}
