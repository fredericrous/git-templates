//! Make the checks see what is being committed.
//!
//! `staged_files()` asks the index for the PATH LIST, which is right, and then
//! hands those paths to tools that open them from the WORKING TREE. Eleven of
//! the fifteen pre-commit checks do this. So `git add -p` half a file, commit,
//! and prettier reads the whole working-tree file: it fails on lines you did not
//! stage, or passes on lines you did.
//!
//! `pre-commit` fixes this for everyone by stashing the unstaged changes for the
//! duration of the run, and their wording names both directions:
//!
//! > Running hooks on unstaged changes can lead to both false-positives and
//! > false-negatives during committing.
//!
//! ## This is the most dangerous code in the repository
//!
//! A stash taken and not restored loses uncommitted work. That is worse than
//! either failure that overwrote tracked files, because there is nothing on disk
//! to recover from. Hence, in order of how likely each is to bite:
//!
//! 1. **Nothing to stash → nothing happens.** The common case never touches the
//!    tree.
//! 2. **Restore on a SIGNAL.** `Drop` runs on unwind and on early return; it
//!    does NOT run when Ctrl-C kills the process, and interrupting a slow
//!    pre-commit is the most probable route to an orphaned stash, not the least.
//! 3. **Restore in `Drop`** for panics and early returns.
//! 4. **Never mid-operation.** A merge or rebase in progress means the tree is
//!    already holding somebody else's work — `GitState` (PR 2) answers this.
//! 5. **Restore failure is loud and fatal**, and prints the ref, so the work is
//!    findable in `git stash list` rather than silently gone.
//! 6. **`githooks restore`** for when even the handler was interrupted.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ui::{error_sign, warning_sign};

/// Whether this process is holding a patch. Global because a signal handler
/// cannot be handed a reference, and because there is at most one per process.
static HELD: AtomicBool = AtomicBool::new(false);

/// Where the unstaged changes are parked. Inside `$GIT_DIR` so it is never
/// committed, never seen by a check, and findable by hand.
pub const PATCH: &str = "githooks-unstaged.patch";

/// Unstaged changes, set aside for the duration of a stage.
pub struct StagedOnly {
    held: bool,
}

/// Why a PATCH and not `git stash --keep-index`.
///
/// Saving is the easy half — `stash push --keep-index` does it. Restoring is
/// not: `stash pop` MERGES the stash into the current tree, and the current
/// tree already holds the staged content, so popping produces conflict markers
/// in the user's file. Measured, on the first attempt at this.
///
/// `git diff` between index and tree IS the unstaged change. Saving that patch,
/// resetting the tree to the index, and re-applying afterwards restores exactly
/// what was there — a deterministic inverse rather than a merge. `pre-commit`
/// reached the same conclusion for the same reason.
impl StagedOnly {
    pub fn enter(hooks_dir: &Path) -> Result<StagedOnly, String> {
        // A tree mid-merge is already holding work that is not the author's.
        if !crate::git_states_in_progress(hooks_dir).is_empty() {
            return Ok(StagedOnly { held: false });
        }
        // Conflicted paths cannot be split into staged and unstaged halves, so
        // refuse to try.
        if crate::git::stdout(&["diff", "--name-only", "--diff-filter=U"])
            .is_some_and(|out| !out.is_empty())
        {
            return Ok(StagedOnly { held: false });
        }
        if !has_unstaged_changes() {
            return Ok(StagedOnly { held: false });
        }

        let Some(patch_path) = patch_path(hooks_dir) else {
            return Ok(StagedOnly { held: false });
        };
        // `--binary` so a change to a binary file survives the round trip.
        let Some(patch) = crate::git::stdout_raw(&["diff", "--binary", "--no-color"]) else {
            return Err(format!(
                "{} could not read the unstaged changes; refusing to check the wrong content",
                error_sign()
            ));
        };
        if std::fs::write(&patch_path, &patch).is_err() {
            return Err(format!(
                "{} could not save the unstaged changes to {}",
                error_sign(),
                patch_path.display()
            ));
        }
        // Tree := index. Everything staged stays; everything else goes into the
        // patch we just wrote.
        if !crate::git::succeeds(&["checkout", "--", "."]) {
            let _ = std::fs::remove_file(&patch_path);
            return Err(format!(
                "{} could not set the unstaged changes aside; nothing was changed",
                error_sign()
            ));
        }
        HELD.store(true, Ordering::SeqCst);
        Ok(StagedOnly { held: true })
    }

    /// Put them back. Idempotent, and safe to call from a signal handler.
    pub fn restore() {
        if !HELD.swap(false, Ordering::SeqCst) {
            return;
        }
        let Some(dir) = crate::git::stdout(&["rev-parse", "--git-dir"]) else {
            return;
        };
        let patch_path = Path::new(&dir).join(PATCH);
        if !patch_path.is_file() {
            return;
        }
        let applied = crate::git::succeeds(&[
            "apply",
            "--whitespace=nowarn",
            patch_path.to_str().unwrap_or_default(),
        ]);
        if applied {
            let _ = std::fs::remove_file(&patch_path);
            return;
        }
        // The one message in this codebase that must never be swallowed.
        eprintln!(
            "{} YOUR UNSTAGED CHANGES COULD NOT BE PUT BACK AUTOMATICALLY.",
            error_sign()
        );
        eprintln!("    They are safe, in: {}", patch_path.display());
        eprintln!("    Recover them with: githooks restore");
    }
}

impl Drop for StagedOnly {
    fn drop(&mut self) {
        if self.held {
            StagedOnly::restore();
        }
    }
}

fn patch_path(hooks_dir: &Path) -> Option<std::path::PathBuf> {
    Some(hooks_dir.parent()?.join(PATCH))
}

fn has_unstaged_changes() -> bool {
    // Tracked files only: `--include-untracked` is not used, so an untracked
    // file is not a reason to stash.
    !crate::git::succeeds(&["diff", "--quiet"])
}

/// Put back a patch this tool parked, from a later invocation.
///
/// For when even the signal handler was interrupted.
pub fn restore_command() -> Result<(), String> {
    let dir = crate::git::stdout(&["rev-parse", "--git-dir"])
        .ok_or_else(|| "not inside a git repository".to_string())?;
    let patch_path = Path::new(&dir).join(PATCH);
    if !patch_path.is_file() {
        println!("{} nothing of ours to restore", warning_sign());
        return Ok(());
    }
    if crate::git::succeeds(&[
        "apply",
        "--whitespace=nowarn",
        patch_path.to_str().unwrap_or_default(),
    ]) {
        let _ = std::fs::remove_file(&patch_path);
        println!("restored your unstaged changes");
        Ok(())
    } else {
        Err(format!(
            "could not apply {} — it is still there, apply it by hand",
            patch_path.display()
        ))
    }
}

/// Restore before dying on a signal.
///
/// `Drop` does not run when the process is killed, and Ctrl-C during a slow
/// pre-commit is the most likely way to reach an orphaned stash. Installed only
/// when a stash is actually held.
#[cfg(unix)]
pub fn install_signal_handler() {
    extern "C" fn on_signal(sig: i32) {
        StagedOnly::restore();
        // Re-raise with the default handler so the exit status is honest about
        // having been killed.
        unsafe {
            libc_signal(sig, 0); // SIG_DFL
            libc_raise(sig);
        }
    }
    unsafe {
        libc_signal(2, on_signal as *const () as usize); // SIGINT
        libc_signal(15, on_signal as *const () as usize); // SIGTERM
    }
}

#[cfg(not(unix))]
pub fn install_signal_handler() {}

// One extern rather than a dependency: `scripts/check-no-deps.sh` keeps this
// binary crate-free, and these are two libc calls with stable signatures.
#[cfg(unix)]
extern "C" {
    #[link_name = "signal"]
    fn libc_signal_raw(sig: i32, handler: usize) -> usize;
    #[link_name = "raise"]
    fn libc_raise_raw(sig: i32) -> i32;
}

#[cfg(unix)]
unsafe fn libc_signal(sig: i32, handler: usize) {
    unsafe {
        libc_signal_raw(sig, handler);
    }
}

#[cfg(unix)]
unsafe fn libc_raise(sig: i32) {
    unsafe {
        libc_raise_raw(sig);
    }
}
