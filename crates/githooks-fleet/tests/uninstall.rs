//! `githooks-fleet uninstall` deletes shims independently of `fix`/`apply` —
//! it is a third code path that reimplements "delete our files" — so it must
//! repeat the guard those two treat as mandatory: never delete a path git
//! tracks, however `.git/hooks` was reached. `fix.rs`'s doc explains why that
//! guard exists at all: `~/.config/git/git-templates` is commonly a SYMLINK to
//! a checkout, and an install/uninstall step that only checked "is this ours"
//! and not "is this tracked" destroyed tracked source before.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn shim_text() -> String {
    std::fs::read_to_string(repo_root().join("templates/hooks/pre-commit"))
        .expect("template")
        .replace("__GITHOOKS_BIN__", "/bin/fake-githooks")
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(unix)]
#[test]
fn uninstall_refuses_a_shim_reached_through_a_symlink_into_a_tracked_checkout() {
    let root = std::env::temp_dir().join(format!("fleet-uninstall-symlink-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("mkdir");

    // A real checkout, unrelated to the fleet root, that TRACKS what looks
    // like a hooks directory — exactly the shape of `git-templates` itself.
    let checkout = root.join("checkout");
    std::fs::create_dir_all(checkout.join("hooks")).expect("mkdir");
    git(&checkout, &["init", "-q", "--template=", "."]);
    git(&checkout, &["config", "user.email", "t@t"]);
    git(&checkout, &["config", "user.name", "t"]);
    std::fs::write(checkout.join("hooks/pre-commit"), shim_text()).expect("write");
    git(&checkout, &["add", "hooks/pre-commit"]);
    git(
        &checkout,
        &["commit", "-q", "--no-verify", "-m", "chore: seed"],
    );

    // A "repository" in the fleet whose `.git` is a SYMLINK into that
    // checkout — so `<victim>/.git/hooks/pre-commit` physically IS the
    // tracked file above, even though nothing under `<victim>` says so.
    let victim = root.join("victim");
    std::fs::create_dir_all(&victim).expect("mkdir");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&checkout, victim.join(".git")).expect("symlink");

    let out = Command::new(env!("CARGO_BIN_EXE_githooks-fleet"))
        .args(["uninstall", "--root"])
        .arg(&root)
        .output()
        .expect("githooks-fleet uninstall");
    assert!(out.status.success(), "{:?}", out);

    let tracked_still_there = std::fs::read_to_string(checkout.join("hooks/pre-commit"))
        .map(|c| c == shim_text())
        .unwrap_or(false);
    assert!(
        tracked_still_there,
        "uninstall deleted a shim that was tracked source reached through a symlink:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// The ordinary case, unaffected: an untracked shim in a plain (non-symlinked)
/// repo is still removed.
#[test]
fn uninstall_still_removes_an_ordinary_untracked_shim() {
    let root = std::env::temp_dir().join(format!("fleet-uninstall-plain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let hooks = root.join("victim/.git/hooks");
    std::fs::create_dir_all(&hooks).expect("mkdir");
    std::fs::write(hooks.join("pre-commit"), shim_text()).expect("write");

    let out = Command::new(env!("CARGO_BIN_EXE_githooks-fleet"))
        .args(["uninstall", "--root"])
        .arg(&root)
        .output()
        .expect("githooks-fleet uninstall");
    assert!(out.status.success(), "{:?}", out);
    assert!(
        !hooks.join("pre-commit").exists(),
        "an ordinary, untracked shim must still be removed"
    );

    let _ = std::fs::remove_dir_all(&root);
}
