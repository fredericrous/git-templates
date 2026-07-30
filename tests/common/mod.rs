//! A throwaway git repo, for driving hooks from Rust.
//!
//! Phase 4 of docs/rust-migration.md. The zsh suites were the migration's
//! harness and stayed untouched on purpose while the hooks moved; now that the
//! hooks are Rust, the suites are the LAST thing requiring zsh — which is why
//! the Windows job can only run a smoke instead of the real tests.
//!
//! Each `Repo` is an isolated temp repository, cleaned up on drop, so tests run
//! in parallel. The old runner created one repo per SUITE and ran cases
//! sequentially inside it, which is why several cases depended on state left by
//! earlier ones (and why one of them could not fail — see pull-rebase).

#![allow(dead_code)] // each integration test binary uses a different subset

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub struct Repo {
    pub dir: PathBuf,
}

/// A counter, so parallel tests never collide on a directory name.
fn unique() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!(
        "githooks-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

impl Repo {
    /// A fresh repo with an identity and NO hooks installed.
    ///
    /// `--template=` (empty) matters: without it `git init` copies the
    /// machine's own hooks in, and a test would exercise those instead of the
    /// binary under test. That bit the Windows smoke before it was noticed.
    pub fn new() -> Self {
        let dir = std::env::temp_dir().join(unique());
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp repo");
        let r = Repo { dir };
        r.git(&["init", "-q", "--template=", "."]);
        r.git(&["config", "user.email", "test@example.com"]);
        r.git(&["config", "user.name", "test"]);
        // Keep the tests independent of the developer's global config.
        r.git(&["config", "commit.gpgsign", "false"]);
        r.git(&["config", "init.defaultBranch", "main"]);
        r
    }

    pub fn git(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(&self.dir).stdin(Stdio::null());
        Self::strip_git_env_impl(&mut cmd);
        cmd.output().expect("run git")
    }

    /// Write a file (creating parents) and stage it.
    pub fn stage(&self, path: &str, content: &str) {
        let full = self.dir.join(path);
        if let Some(p) = full.parent() {
            std::fs::create_dir_all(p).expect("create parent");
        }
        std::fs::write(&full, content).expect("write file");
        self.git(&["add", path]);
    }

    pub fn write(&self, path: &str, content: &str) {
        let full = self.dir.join(path);
        if let Some(p) = full.parent() {
            std::fs::create_dir_all(p).expect("create parent");
        }
        std::fs::write(&full, content).expect("write file");
    }

    pub fn commit(&self, msg: &str) {
        self.git(&["commit", "-q", "--no-verify", "-m", msg]);
    }

    /// Remove git's exported environment.
    ///
    /// These tests are themselves run by `pre-push-cargo-test`, and git gives a
    /// hook GIT_DIR/GIT_INDEX_FILE/GIT_WORK_TREE pointing at the REAL repo.
    /// Those beat `current_dir`, so without this a fixture's `git commit`
    /// commits to git-templates itself. It did exactly that once.
    fn strip_git_env_impl(cmd: &mut Command) {
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().starts_with("GIT_") {
                cmd.env_remove(&k);
            }
        }
    }

    /// Run a hook through the binary, as the shim would.
    pub fn hook(&self, name: &str, args: &[&str]) -> HookRun {
        let mut cmd = Command::new(bin());
        cmd.arg("--hooks-dir")
            .arg(self.dir.join(".git/hooks"))
            .arg(name)
            .args(args)
            .current_dir(&self.dir)
            .stdin(Stdio::null());
        Self::strip_git_env_impl(&mut cmd);
        let out = cmd.output().expect("run githooks");
        HookRun {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub struct HookRun {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl HookRun {
    pub fn passed(&self) -> bool {
        self.code == 0
    }
    /// Everything the hook printed — several hooks report on stderr.
    pub fn output(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
    pub fn says(&self, needle: &str) -> bool {
        self.output().contains(needle)
    }
    /// No output at all: several hooks must be SILENT when out of scope, and
    /// "exit 0" alone does not distinguish that from "ran and approved".
    pub fn silent(&self) -> bool {
        self.output().trim().is_empty()
    }
}

#[cfg(unix)]
fn make_executable(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
}

/// Windows has no executable bit; the dispatcher runs scripts through their
/// shebang interpreter there, so nothing is needed.
#[cfg(not(unix))]
fn make_executable(_p: &Path) {}

/// The binary under test. Cargo builds it before integration tests and points
/// at it via CARGO_BIN_EXE_*.
fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_githooks")
}

/// True when a tool the case genuinely needs is absent — and it SAYS SO.
///
/// Rust has no native skip: an early `return` reports as a pass, which is the
/// exact trap the zsh suites already guarded against by printing
/// "unavailable — skipping". The same phrase is used here so CI's
/// skip-reporter sees both harnesses, and a suite that quietly did not run
/// cannot masquerade as one that passed.
pub fn missing(tool: &str) -> bool {
    let found = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path).any(|d| {
                d.join(tool).is_file()
                    || (cfg!(windows)
                        && [".exe", ".cmd", ".bat"]
                            .iter()
                            .any(|e| d.join(format!("{tool}{e}")).is_file()))
            })
        })
        .unwrap_or(false);
    if !found {
        println!("  ! {tool} unavailable — skipping");
    }
    !found
}

/// Absolute path to the repo's own templates/hooks, for tests that need a shim.
pub fn template_hook(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("templates/hooks")
        .join(name)
}
