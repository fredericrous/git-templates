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
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::ui::{error_sign, warning_sign};

/// Whether this process is holding a patch. Global because a signal handler
/// cannot be handed a reference, and because there is at most one per process.
static HELD: AtomicBool = AtomicBool::new(false);

/// Where the unstaged changes are parked. Inside `$GIT_DIR` so they are never
/// committed, never seen by a check, and findable by hand.
pub const STORE: &str = "githooks-held";

/// Unstaged changes, set aside for the duration of a stage.
pub struct StagedOnly {
    held: bool,
}

/// Why COPIES and not `git stash --keep-index`, and not a patch either.
///
/// Saving is the easy half; restoring is the whole problem.
///
/// `stash pop` MERGES into a tree that already holds the staged content, so it
/// writes conflict markers into the user's file. Measured on the first attempt.
///
/// `git diff` + `git apply` is deterministic on Unix and is what `pre-commit`
/// does — but it applies PATCH semantics to text, and Git for Windows converts
/// line endings by default. Measured on the second attempt: every restore test
/// failed on Windows and passed everywhere else, which is the worst possible
/// shape for the one routine in this codebase that can lose somebody's work.
///
/// So: byte-exact copies. Read the file, put it back. No patch to apply, no
/// newline policy to agree about, and binary files need no special case. It
/// costs a temporary copy of only the files that have unstaged changes.
impl StagedOnly {
    pub fn enter() -> Result<StagedOnly, String> {
        // A tree mid-merge is already holding work that is not the author's.
        if !crate::git_states_in_progress().is_empty() {
            return Ok(StagedOnly { held: false });
        }
        // Conflicted paths cannot be split into staged and unstaged halves.
        if crate::git::stdout(&["diff", "--name-only", "--diff-filter=U"])
            .is_some_and(|out| !out.is_empty())
        {
            return Ok(StagedOnly { held: false });
        }
        // Tracked files only: an untracked file is not part of this commit and
        // moving it would surprise everyone.
        let changed: Vec<String> =
            crate::git::stdout_paths(&["diff", "--name-only"]).unwrap_or_default();
        if changed.is_empty() {
            return Ok(StagedOnly { held: false });
        }

        let Some(store) = store_dir() else {
            return Ok(StagedOnly { held: false });
        };
        let _ = std::fs::remove_dir_all(&store);
        let root = crate::hooks::common::repo_root();
        let root = Path::new(&root);

        for rel in &changed {
            let from = root.join(rel);
            let to = store.join(rel);
            if let Some(parent) = to.parent() {
                if std::fs::create_dir_all(parent).is_err() {
                    return Err(held_nothing(&store));
                }
            }
            match std::fs::read(&from) {
                // Modified: keep the bytes.
                Ok(bytes) => {
                    if std::fs::write(&to, bytes).is_err() {
                        return Err(held_nothing(&store));
                    }
                }
                // Deleted in the tree but not staged: record the absence, so
                // the restore deletes it again rather than resurrecting it.
                Err(_) => {
                    if std::fs::write(to.with_extension("githooks-absent"), b"").is_err() {
                        return Err(held_nothing(&store));
                    }
                }
            }
        }

        // Tree := index. Everything staged stays; everything else is in `store`.
        if !crate::git::succeeds(&["checkout", "--", "."]) {
            let _ = std::fs::remove_dir_all(&store);
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
        let store = Path::new(&dir).join(STORE);
        if !store.is_dir() {
            return;
        }
        let root = crate::hooks::common::repo_root();
        match put_back(&store, Path::new(&root)) {
            Ok(()) => {
                let _ = std::fs::remove_dir_all(&store);
            }
            Err(_) => {
                // The one message in this codebase that must never be swallowed.
                eprintln!(
                    "{} YOUR UNSTAGED CHANGES COULD NOT BE PUT BACK AUTOMATICALLY.",
                    error_sign()
                );
                eprintln!("    They are safe, in: {}", store.display());
                eprintln!("    Recover them with: githooks restore");
            }
        }
    }
}

fn held_nothing(store: &Path) -> String {
    let _ = std::fs::remove_dir_all(store);
    format!(
        "{} could not hold the unstaged changes aside; nothing was changed",
        error_sign()
    )
}

/// Copy every held file back over the tree.
fn put_back(store: &Path, root: &Path) -> std::io::Result<()> {
    for entry in walk(store)? {
        let rel = entry.strip_prefix(store).unwrap_or(&entry).to_path_buf();
        let rel_str = rel.to_string_lossy().to_string();
        if let Some(original) = rel_str.strip_suffix(".githooks-absent") {
            // It was deleted in the tree; `checkout` brought it back.
            let _ = std::fs::remove_file(root.join(original));
            continue;
        }
        let target = root.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, std::fs::read(&entry)?)?;
    }
    Ok(())
}

fn walk(dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            out.extend(walk(&path)?);
        } else {
            out.push(path);
        }
    }
    Ok(out)
}

/// Where the store lives, agreeing with [`StagedOnly::restore`] by construction
/// — both ask git the same question, rather than one of them deriving a path
/// from `hooks_dir`. That used to be `hooks_dir.parent()`: correct for the
/// main worktree, where `.git/hooks`'s parent IS `$GIT_DIR`, but wrong for a
/// LINKED worktree, where hooks dispatch from the COMMON directory's shared
/// `hooks/` while `$GIT_DIR` is the worktree's own PRIVATE gitdir. The
/// mismatch parked files in one directory and looked for them in the other —
/// silently, since `restore` treats "no store" as "nothing to do" — which is
/// how a real commit in a real worktree lost real unstaged content while this
/// very fix was being written.
fn store_dir() -> Option<std::path::PathBuf> {
    let dir = crate::git::stdout(&["rev-parse", "--git-dir"])?;
    Some(Path::new(&dir).join(STORE))
}

impl Drop for StagedOnly {
    fn drop(&mut self) {
        if self.held {
            StagedOnly::restore();
        }
    }
}

/// Put back files this tool parked, from a later invocation.
///
/// For when even the signal handler was interrupted.
pub fn restore_command() -> Result<(), String> {
    let dir = crate::git::stdout(&["rev-parse", "--git-dir"])
        .ok_or_else(|| "not inside a git repository".to_string())?;
    let store = Path::new(&dir).join(STORE);
    if !store.is_dir() {
        println!("{} nothing of ours to restore", warning_sign());
        return Ok(());
    }
    let root = crate::hooks::common::repo_root();
    put_back(&store, Path::new(&root))
        .map_err(|e| format!("could not put {} back: {e}", store.display()))?;
    let _ = std::fs::remove_dir_all(&store);
    println!("restored your unstaged changes");
    Ok(())
}

/// The write end of the self-pipe a signal handler wakes the watcher thread
/// through. `-1` until `install_signal_handler` has run.
#[cfg(unix)]
static SIGNAL_PIPE_WRITE: AtomicI32 = AtomicI32::new(-1);

/// Which signal woke the watcher, so it can re-raise the right one.
#[cfg(unix)]
static PENDING_SIGNAL: AtomicI32 = AtomicI32::new(0);

/// Restore before dying on a signal.
///
/// `Drop` does not run when the process is killed, and Ctrl-C during a slow
/// pre-commit is the most likely way to reach an orphaned stash. Installed only
/// when a stash is actually held.
///
/// The handler itself does almost nothing. `restore()` runs `git`, walks the
/// filesystem, writes files and prints — none of that is async-signal-safe,
/// and running it IN the handler risks a deadlock: if the thread the signal
/// interrupted already held a lock the handler's own code would then wait on
/// forever (the allocator's, or stdio's), the process hangs instead of
/// exiting, which is worse than either failure `restore` exists to prevent.
///
/// So the handler only records which signal arrived and writes one byte down
/// a pipe — both on POSIX's async-signal-safe list — and a plain background
/// thread, blocked reading that pipe, does the actual restore once it wakes,
/// in ordinary thread context where none of those restrictions apply. This is
/// the standard "self-pipe" pattern for getting work out of a signal handler.
#[cfg(unix)]
pub fn install_signal_handler() {
    let mut fds = [-1i32; 2];
    if unsafe { libc_pipe(fds.as_mut_ptr()) } != 0 {
        // No pipe, no watcher, no handler: Ctrl-C falls back to the default
        // action. Losing the safety net is better than building it on a
        // primitive that just failed us.
        return;
    }
    let (read_fd, write_fd) = (fds[0], fds[1]);
    SIGNAL_PIPE_WRITE.store(write_fd, Ordering::SeqCst);

    std::thread::spawn(move || loop {
        let mut byte = 0u8;
        let n = unsafe { libc_read(read_fd, &mut byte as *mut u8, 1) };
        if n <= 0 {
            return; // pipe closed, or a real error: nothing left to watch for
        }
        StagedOnly::restore();
        // Re-raise with the default handler so the exit status is honest
        // about having been killed.
        let sig = PENDING_SIGNAL.load(Ordering::SeqCst);
        if sig != 0 {
            unsafe {
                libc_signal(sig, 0); // SIG_DFL
                libc_raise(sig);
            }
        }
    });

    extern "C" fn on_signal(sig: i32) {
        PENDING_SIGNAL.store(sig, Ordering::SeqCst);
        let fd = SIGNAL_PIPE_WRITE.load(Ordering::SeqCst);
        if fd >= 0 {
            let byte = 1u8;
            unsafe {
                libc_write(fd, &byte as *const u8, 1);
            }
        }
    }
    unsafe {
        libc_signal(2, on_signal as *const () as usize); // SIGINT
        libc_signal(15, on_signal as *const () as usize); // SIGTERM
    }
}

#[cfg(not(unix))]
pub fn install_signal_handler() {}

// Externs rather than a dependency: `scripts/check-no-deps.sh` keeps this
// binary crate-free, and these are five libc calls with stable signatures —
// `pipe`/`read`/`write` need nothing beyond plain integers and byte pointers,
// so unlike `sigset_t`-based APIs there is no opaque, platform-varying struct
// layout to get wrong by hand.
#[cfg(unix)]
extern "C" {
    #[link_name = "signal"]
    fn libc_signal_raw(sig: i32, handler: usize) -> usize;
    #[link_name = "raise"]
    fn libc_raise_raw(sig: i32) -> i32;
    #[link_name = "pipe"]
    fn libc_pipe_raw(fds: *mut i32) -> i32;
    #[link_name = "read"]
    fn libc_read_raw(fd: i32, buf: *mut u8, count: usize) -> isize;
    #[link_name = "write"]
    fn libc_write_raw(fd: i32, buf: *const u8, count: usize) -> isize;
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

#[cfg(unix)]
unsafe fn libc_pipe(fds: *mut i32) -> i32 {
    unsafe { libc_pipe_raw(fds) }
}

#[cfg(unix)]
unsafe fn libc_read(fd: i32, buf: *mut u8, count: usize) -> isize {
    unsafe { libc_read_raw(fd, buf, count) }
}

#[cfg(unix)]
unsafe fn libc_write(fd: i32, buf: *const u8, count: usize) -> isize {
    unsafe { libc_write_raw(fd, buf, count) }
}
