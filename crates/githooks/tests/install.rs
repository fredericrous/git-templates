//! `githooks install`, driven through the real binary.
//!
//! The guard is unit-tested next to itself; these cover what a unit test cannot:
//! that the subcommand is reachable at all, that it writes what it claims, and
//! that the refusal path really does leave a checkout untouched — the failure
//! that has already happened twice.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        let d = std::env::temp_dir().join(format!("gh-inst-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("mkdir");
        Sandbox(d)
    }
    /// Joined component by component, NOT `join("a/b")`.
    ///
    /// On Windows `join` keeps a forward slash it was handed, so the one-string
    /// form yields `C:\tmp\sandbox\.local/bin\githooks.exe` — the same file
    /// as the installer's path, spelled differently. Comparing the two as
    /// strings then fails for a reason that has nothing to do with the product.
    fn path(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.0.clone(), |p, c| p.join(c))
    }
    /// Run the installer with HOME and XDG pointed inside the sandbox, so a bug
    /// cannot reach the developer's real configuration.
    fn install(&self, cwd: &Path) -> (i32, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_githooks"))
            .arg("install")
            .current_dir(cwd)
            .env("HOME", &self.0)
            .env("USERPROFILE", &self.0)
            .env("XDG_CONFIG_HOME", self.path(".config"))
            .output()
            .expect("run githooks install");
        (
            out.status.code().unwrap_or(-1),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git");
}

fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).expect("mkdir");
    git(dir, &["init", "-q", "--template=", "."]);
    git(dir, &["config", "user.email", "t@t.test"]);
    git(dir, &["config", "user.name", "t"]);
}

/// The binary name the installer chose, whichever platform this is.
fn installed_bin(s: &Sandbox) -> PathBuf {
    let dir = s.path(".local/bin");
    for n in ["githooks", "githooks.exe"] {
        let p = dir.join(n);
        if p.is_file() {
            return p;
        }
    }
    panic!("no binary installed into {}", dir.display());
}

#[test]
fn install_places_the_binary_the_templates_and_the_repo_hooks() {
    let s = Sandbox::new("full");
    let repo = s.path("repo");
    init_repo(&repo);

    let (code, out) = s.install(&repo);
    assert_eq!(code, 0, "{out}");

    let bin = installed_bin(&s);
    let tpl = s.path(".config/git/git-templates/templates/hooks");
    for name in ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"] {
        assert!(tpl.join(name).is_file(), "template {name} missing:\n{out}");
        assert!(
            repo.join(".git/hooks").join(name).is_file(),
            "repo hook {name} missing:\n{out}"
        );
    }

    // Baked, not merely copied: the placeholder is what a GUI client cannot
    // resolve, and leaving it would be the bug the baking exists to prevent.
    let hook = std::fs::read_to_string(repo.join(".git/hooks/pre-commit")).expect("read");
    assert!(!hook.contains("__GITHOOKS_BIN__"), "shim was not baked");
    assert!(
        hook.contains(bin.to_str().expect("utf8")),
        "shim names {:?}, not the installed {}",
        hook.lines().find(|l| l.contains("githooks")),
        bin.display()
    );
}

/// Running it twice must be a no-op, not an error — it is the normal way to
/// pick up a new build.
#[test]
fn installing_twice_is_idempotent() {
    let s = Sandbox::new("twice");
    let repo = s.path("repo");
    init_repo(&repo);

    assert_eq!(s.install(&repo).0, 0);
    let first = std::fs::read_to_string(repo.join(".git/hooks/pre-commit")).expect("read");
    let (code, out) = s.install(&repo);
    assert_eq!(code, 0, "{out}");
    let second = std::fs::read_to_string(repo.join(".git/hooks/pre-commit")).expect("read");
    assert_eq!(first, second, "a second install changed the shim");
}

/// The incident, reproduced: the XDG path is a SYMLINK to a checkout, so
/// "installing" there means overwriting tracked source. It must refuse, and say
/// so, and leave every tracked file exactly as it was.
#[test]
fn a_symlinked_checkout_is_refused_and_left_untouched() {
    let s = Sandbox::new("symlink");
    let checkout = s.path("checkout");
    init_repo(&checkout);
    let hooks = checkout.join("templates/hooks");
    std::fs::create_dir_all(&hooks).expect("mkdir");
    std::fs::write(hooks.join("pre-commit"), "precious __GITHOOKS_BIN__\n").expect("write");
    git(&checkout, &["add", "-A"]);
    git(&checkout, &["commit", "-qm", "seed"]);

    let cfg = s.path(".config/git");
    std::fs::create_dir_all(&cfg).expect("mkdir");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&checkout, cfg.join("git-templates")).expect("symlink");
    #[cfg(windows)]
    {
        // Windows may refuse without Developer Mode; the guard is unit-tested
        // there, and this case is about the symlink specifically.
        if std::os::windows::fs::symlink_dir(&checkout, cfg.join("git-templates")).is_err() {
            return;
        }
    }

    let before = std::fs::read_to_string(hooks.join("pre-commit")).expect("read");
    let repo = s.path("repo");
    init_repo(&repo);
    let (code, out) = s.install(&repo);

    assert_eq!(code, 0, "refusing is not an error:\n{out}");
    assert!(
        out.contains("IS the checkout"),
        "the refusal must be explained:\n{out}"
    );
    let after = std::fs::read_to_string(hooks.join("pre-commit")).expect("read");
    assert_eq!(before, after, "TRACKED FILE WAS REWRITTEN");
    assert!(
        after.contains("__GITHOOKS_BIN__"),
        "the tracked placeholder was baked away"
    );
}

/// Installing from outside any repository must still install the binary and the
/// templates, and say plainly that it wrote no repo hooks.
#[test]
fn outside_a_repository_it_installs_what_it_can() {
    let s = Sandbox::new("norepo");
    let plain = s.path("plain");
    std::fs::create_dir_all(&plain).expect("mkdir");

    let (code, out) = s.install(&plain);
    assert_eq!(code, 0, "{out}");
    installed_bin(&s);
    assert!(
        out.contains("not inside a git repository"),
        "should say why no repo hooks were written:\n{out}"
    );
}

/// Refusing to touch a checkout is success; FAILING a write it was allowed to
/// make is not. Reporting success after a step did not happen is the failure
/// this whole codebase is arranged against.
#[cfg(unix)]
#[test]
fn a_template_dir_it_cannot_write_fails_the_install() {
    use std::os::unix::fs::PermissionsExt;
    // root ignores the permission bits and the test would prove nothing.
    if unsafe { libc_geteuid() } == 0 {
        return;
    }
    let s = Sandbox::new("readonly");
    let repo = s.path("repo");
    init_repo(&repo);
    let tpl = s.path(".config/git/git-templates/templates/hooks");
    std::fs::create_dir_all(&tpl).expect("mkdir");
    std::fs::set_permissions(&tpl, std::fs::Permissions::from_mode(0o555)).expect("chmod");

    let (code, out) = s.install(&repo);
    // Restore before the sandbox is dropped, or cleanup cannot remove it.
    let _ = std::fs::set_permissions(&tpl, std::fs::Permissions::from_mode(0o755));

    assert_ne!(code, 0, "a failed write reported success:\n{out}");
    assert!(
        out.contains("cannot write shims"),
        "and did not say what failed:\n{out}"
    );
}

#[cfg(unix)]
unsafe fn libc_geteuid() -> u32 {
    // One extern rather than a dependency: the commit path's crate graph is
    // guarded, and this is a test-only three-liner.
    extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

/// The installed shim has to actually work — a hook that is written but cannot
/// run is the failure this whole shim design is arranged against.
#[test]
fn the_installed_hooks_run_a_real_commit() {
    let s = Sandbox::new("commit");
    let repo = s.path("repo");
    init_repo(&repo);
    assert_eq!(s.install(&repo).0, 0);

    std::fs::write(repo.join("a.txt"), "hello\n").expect("write");
    git(&repo, &["add", "-A"]);
    let out = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["commit", "-m", "feat: a commit through installed hooks"])
        .env("HOME", &s.0)
        .output()
        .expect("commit");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "commit failed:\n{text}");
    assert!(
        !text.contains("githooks binary not found"),
        "the installed shim could not resolve the binary:\n{text}"
    );
}
