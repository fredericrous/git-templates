//! Run a pre-push check against what is being PUSHED.
//!
//! `rust-test` and `run-tests-js` take their file set from the pushed refs —
//! correct — and then run the suite with `current_dir` set to the developer's
//! working tree. So the suite can pass on an uncommitted fix, or fail on an
//! uncommitted experiment, and in neither case has it tested the commits being
//! pushed. Same gap as the pre-commit one, from the other end.
//!
//! ## Why not the stash
//!
//! Holding unstaged changes aside is the pre-commit answer, and it is the wrong
//! instrument here. A push is not a staging operation: the difference that
//! matters is not tree-versus-index but tree-versus-the-commit-you-are-sending,
//! and that includes everything staged-but-uncommitted too. Setting all of it
//! aside for the length of a test suite would leave the developer looking at a
//! tree that is not theirs for minutes at a time.
//!
//! ## What it costs, which is the whole question
//!
//! `git worktree add --detach <tip>` materialises the pushed commit somewhere
//! else and the suite runs there. The tree is untouched, and an interrupted run
//! leaves a worktree rather than a mangled checkout.
//!
//! The cost is real: a second checkout, and a build that cannot reuse the
//! primary tree's `target/` cache, so the first push after this lands is a cold
//! build. That is why it is opt-in — `git config githooks.testPushedTree true`
//! — rather than the default. The default keeps today's behaviour and now SAYS
//! what it is testing, which was the actual bug: not that it used the tree, but
//! that nobody knew it did.

use std::path::{Path, PathBuf};

use crate::ui::warning_sign;

/// Whether the user asked for the accurate-but-slower answer.
pub fn enabled() -> bool {
    matches!(
        crate::git::stdout(&["config", "--get", "githooks.testPushedTree"]).as_deref(),
        Some("true") | Some("1") | Some("yes")
    )
}

/// A checkout of the pushed commit, removed when it goes out of scope.
pub struct PushedTree {
    path: PathBuf,
}

impl PushedTree {
    /// Materialise `tip`, or `None` when that is not possible — in which case
    /// the caller falls back to the working tree and says so.
    pub fn create(tip: &str) -> Option<PushedTree> {
        let base = std::env::temp_dir().join(format!("githooks-push-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let path = base.clone();
        let ok = crate::git::succeeds(&[
            "worktree",
            "add",
            "--detach",
            "--quiet",
            path.to_str()?,
            tip,
        ]);
        ok.then_some(PushedTree { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PushedTree {
    fn drop(&mut self) {
        // `--force`: the suite may have written into it, and a build artefact
        // must not be a reason to leave a worktree registered forever.
        let _ = crate::git::succeeds(&[
            "worktree",
            "remove",
            "--force",
            self.path.to_str().unwrap_or_default(),
        ]);
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The commit at the tip of what is being pushed.
pub fn tip(refs: &[crate::pushrefs::PushRef]) -> Option<String> {
    let zero = crate::git::stdout(&["hash-object", "--stdin"])
        .map(|h| "0".repeat(h.len()))
        .unwrap_or_else(|| "0".repeat(40));
    refs.iter()
        .find(|r| r.local_oid != zero)
        .map(|r| r.local_oid.clone())
}

/// Where a pre-push suite should run, and whether that is the honest answer.
///
/// Returns the directory plus the guard that owns it — dropping the guard
/// removes the worktree, so the caller must hold it for the length of the run.
pub fn where_to_run(
    refs: &[crate::pushrefs::PushRef],
    fallback: &str,
) -> (PathBuf, Option<PushedTree>) {
    if !enabled() {
        // Today's behaviour, but no longer silent about it.
        println!(
            "{} testing the WORKING TREE, not the pushed commits \
             (`git config githooks.testPushedTree true` to test what you are pushing)",
            warning_sign()
        );
        return (PathBuf::from(fallback), None);
    }
    let Some(tip) = tip(refs) else {
        return (PathBuf::from(fallback), None);
    };
    match PushedTree::create(&tip) {
        Some(tree) => {
            let path = tree.path().to_path_buf();
            (path, Some(tree))
        }
        None => {
            println!(
                "{} could not check out {tip} to test it; testing the working tree instead",
                warning_sign()
            );
            (PathBuf::from(fallback), None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pushed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for args in [
            vec!["init", "-q", "--template=", "."],
            vec!["config", "user.email", "t@t.test"],
            vec!["config", "user.name", "t"],
        ] {
            std::process::Command::new("git")
                .args(&args)
                .current_dir(&d)
                .output()
                .expect("git");
        }
        d
    }

    /// The point: the checkout holds the COMMIT, not whatever the developer
    /// has open.
    #[test]
    fn the_worktree_holds_the_committed_content() {
        let d = repo("tree");
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&d)
                .output()
                .expect("git")
        };
        std::fs::write(d.join("a.txt"), "committed\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-qm", "seed"]);
        let head = String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();
        // Uncommitted, and it must not travel.
        std::fs::write(d.join("a.txt"), "dirty, not pushed\n").unwrap();

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&d).unwrap();
        let tree = PushedTree::create(&head).expect("worktree");
        let seen = std::fs::read_to_string(tree.path().join("a.txt")).unwrap();
        let at = tree.path().to_path_buf();
        drop(tree);
        std::env::set_current_dir(previous).unwrap();

        assert_eq!(seen, "committed\n", "the worktree saw the dirty tree");
        assert!(!at.exists(), "the worktree outlived its guard");
        let _ = std::fs::remove_dir_all(&d);
    }
}
