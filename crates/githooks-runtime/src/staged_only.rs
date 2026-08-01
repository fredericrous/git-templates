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
use std::sync::Mutex;

use crate::ui::{error_sign, warning_sign};

/// Whether this process is holding a patch. Global because a signal handler
/// cannot be handed a reference, and because there is at most one per process.
static HELD: AtomicBool = AtomicBool::new(false);

/// Guards `checkout` through `HELD.store(true, ..)` in `enter()` against a
/// `restore()` racing in concurrently from the signal-watcher thread.
///
/// Without this, a signal landing mid-`checkout` lets `restore()` read `HELD`
/// as still `false`, no-op, and kill the process — while `checkout`'s child
/// keeps running, ORPHANED, and eventually finishes writing the tree anyway.
/// The parked files are never put back and nothing said so. This is not
/// hypothetical: it is what running the checkout-then-signal race a dozen
/// times over actually produced, once `restore()` moved off the interrupted
/// call stack and onto a thread of its own — see `install_signal_handler`.
/// `restore()` blocking here until `enter()` finishes is exactly the fix: by
/// the time it can read `HELD`, `checkout` has definitely either succeeded
/// (and it restores) or never called (and it no-ops), never "maybe".
static ENTER_LOCK: Mutex<()> = Mutex::new(());

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
    pub fn enter(hooks_dir: &Path) -> Result<StagedOnly, String> {
        // A tree mid-merge is already holding work that is not the author's.
        if !crate::git_states_in_progress(hooks_dir).is_empty() {
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
        let changed: Vec<String> = crate::git::stdout(&["diff", "--name-only"])
            .map(|out| out.lines().map(str::to_owned).collect())
            .unwrap_or_default();
        if changed.is_empty() {
            return Ok(StagedOnly { held: false });
        }

        let Some(store) = store_dir(hooks_dir) else {
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
            // A symlink must be read as a link, not opened: `fs::read` follows
            // it, which copies the TARGET's bytes instead of the link, and
            // silently mistakes a dangling link (a normal mid-edit state) for
            // a deleted file — the absent-marker branch below would then
            // delete the link on restore rather than put it back.
            match std::fs::symlink_metadata(&from) {
                Ok(meta) if meta.file_type().is_symlink() => match std::fs::read_link(&from) {
                    Ok(link_target) => {
                        if std::fs::write(
                            to.with_extension("githooks-symlink"),
                            link_target.to_string_lossy().as_bytes(),
                        )
                        .is_err()
                        {
                            return Err(held_nothing(&store));
                        }
                    }
                    Err(_) => return Err(held_nothing(&store)),
                },
                _ => match std::fs::read(&from) {
                    // Modified: keep the bytes.
                    Ok(bytes) => {
                        if std::fs::write(&to, bytes).is_err() {
                            return Err(held_nothing(&store));
                        }
                    }
                    // Deleted in the tree but not staged: record the absence,
                    // so the restore deletes it again rather than
                    // resurrecting it.
                    Err(_) => {
                        if std::fs::write(to.with_extension("githooks-absent"), b"").is_err() {
                            return Err(held_nothing(&store));
                        }
                    }
                },
            }
        }

        // Tree := index. Everything staged stays; everything else is in
        // `store`. Locked against a concurrent `restore()` — see `ENTER_LOCK`.
        {
            let _guard = ENTER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            if !crate::git::succeeds(&["checkout", "--", "."]) {
                let _ = std::fs::remove_dir_all(&store);
                return Err(format!(
                    "{} could not set the unstaged changes aside; nothing was changed",
                    error_sign()
                ));
            }
            HELD.store(true, Ordering::SeqCst);
        }
        Ok(StagedOnly { held: true })
    }

    /// Put them back. Idempotent, and safe to call from the watcher thread
    /// `install_signal_handler` starts — never from the signal handler itself.
    pub fn restore() {
        // Blocks until `enter()` has definitely finished its own checkout —
        // see `ENTER_LOCK`. The common case (no `enter()` active) is
        // uncontended.
        let _guard = ENTER_LOCK.lock().unwrap_or_else(|p| p.into_inner());
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
        if let Some(original) = rel_str.strip_suffix(".githooks-symlink") {
            let link_target = std::fs::read_to_string(&entry)?;
            let link_path = root.join(original);
            // `checkout` put the staged symlink there; replace it, don't
            // merge with it.
            let _ = std::fs::remove_file(&link_path);
            create_symlink(&link_target, &link_path)?;
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

#[cfg(unix)]
fn create_symlink(target: &str, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Windows distinguishes file and directory symlinks at creation time. The
/// target usually still exists (it was the staged content `checkout` left
/// behind, untouched by this whole dance), so ask it; a dangling link falls
/// back to `symlink_file`, the more common case.
#[cfg(windows)]
fn create_symlink(target: &str, link: &Path) -> std::io::Result<()> {
    let resolved = link
        .parent()
        .map(|parent| parent.join(target))
        .unwrap_or_else(|| Path::new(target).to_path_buf());
    if resolved.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[cfg(not(any(unix, windows)))]
fn create_symlink(target: &str, link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other(format!(
        "no symlink support on this platform: {} -> {target}",
        link.display()
    )))
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

fn store_dir(hooks_dir: &Path) -> Option<std::path::PathBuf> {
    Some(hooks_dir.parent()?.join(STORE))
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

/// Restore before dying on a signal.
///
/// `Drop` does not run when the process is killed, and Ctrl-C during a slow
/// pre-commit is the most likely way to reach an orphaned stash. Called before
/// `enter()`, not after it holds anything — a signal arriving in the gap
/// between `enter()` checking the tree out and this being armed would hit the
/// default disposition instead, and `restore()` no-ops harmlessly on a signal
/// that lands before there is anything held.
///
/// The handler itself does none of the restoring. `restore()` forks `git` and
/// allocates — neither is on POSIX's async-signal-safe list — and `pre-commit`
/// runs checks concurrently on other threads, so a signal delivered while one
/// of them holds the allocator lock (or is itself mid-`fork`) would deadlock
/// the handler instead of running it: the one failure mode worse than an
/// orphaned stash, because it hangs instead of leaving something to recover by
/// hand.
///
/// So the handler does the one thing POSIX actually guarantees is safe here —
/// `write` one byte naming the signal to a pipe — and a plain thread, started
/// up front and blocked on `read`, does the rest once it wakes on an ordinary
/// call stack. It restores, then puts the signal's default disposition back
/// and raises it on itself, so the process still dies BY that signal — an
/// observer still sees an honest exit status, just from a thread that was
/// never inside signal context to begin with.
#[cfg(unix)]
pub fn install_signal_handler() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut fds = [-1i32; 2];
        if unsafe { libc_pipe(fds.as_mut_ptr()) } != 0 {
            // No pipe, no handler: left uncaught, SIGINT/SIGTERM still kill
            // the process (the kernel's default disposition), just without
            // this restore — the same outcome as today if `pipe(2)` ever
            // fails, which in practice it does not.
            return;
        }
        let [read_fd, write_fd] = fds;
        SIGNAL_PIPE_WRITE.store(write_fd, Ordering::SeqCst);

        std::thread::spawn(move || {
            let mut byte = [0u8; 1];
            // Blocks until a handler writes one, or the pipe breaks (only
            // possible if this thread outlives the process, which it cannot).
            if unsafe { libc_read(read_fd, byte.as_mut_ptr(), 1) } <= 0 {
                return;
            }
            StagedOnly::restore();
            let sig = i32::from(byte[0]);
            unsafe {
                libc_signal(sig, 0); // SIG_DFL
                libc_raise(sig);
            }
        });

        extern "C" fn on_signal(sig: i32) {
            let fd = SIGNAL_PIPE_WRITE.load(Ordering::SeqCst);
            if fd >= 0 {
                let byte = sig as u8;
                unsafe {
                    libc_write(fd, &byte, 1);
                }
            }
        }
        unsafe {
            libc_signal(2, on_signal as *const () as usize); // SIGINT
            libc_signal(15, on_signal as *const () as usize); // SIGTERM
        }
    });
}

#[cfg(not(unix))]
pub fn install_signal_handler() {}

/// The pipe's write end, set once by `install_signal_handler` before either
/// signal is armed. `-1` means "no pipe yet" — reachable only if `pipe(2)`
/// itself failed, in which case the handler is never armed either and this is
/// never read.
#[cfg(unix)]
static SIGNAL_PIPE_WRITE: AtomicI32 = AtomicI32::new(-1);

// Raw externs rather than a dependency: `scripts/check-no-deps.sh` keeps this
// binary crate-free, and these are five libc calls with stable signatures.
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

/// # Safety
/// Async-signal-safe: `write(2)` is on POSIX's list, which is the entire
/// reason this exists rather than a call to `restore()` inline.
#[cfg(unix)]
unsafe fn libc_write(fd: i32, buf: *const u8, count: usize) -> isize {
    unsafe { libc_write_raw(fd, buf, count) }
}
