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

/// Hooks are SHARED across every worktree of a repository — git dispatches
/// them from the common directory, never a linked worktree's own private
/// gitdir. An install run from inside a linked worktree must bake shims where
/// git will actually look, not into `.git/worktrees/<name>/hooks`, which git
/// never reads and which would make the install silently inert.
#[test]
fn installing_from_a_linked_worktree_bakes_into_the_shared_hooks_dir() {
    let s = Sandbox::new("worktree");
    let repo = s.path("repo");
    init_repo(&repo);
    git(&repo, &["commit", "--allow-empty", "-qm", "seed"]);
    let wt = s.path("repo-wt");
    let out = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["worktree", "add", "-q", "-b", "feature"])
        .arg(&wt)
        .output()
        .expect("git worktree add");
    assert!(
        out.status.success(),
        "git worktree add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let (code, out) = s.install(&wt);
    assert_eq!(code, 0, "{out}");
    for name in ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"] {
        assert!(
            repo.join(".git/hooks").join(name).is_file(),
            "{name} missing from the common hooks dir:\n{out}"
        );
    }
    // The worktree's own PRIVATE gitdir — `wt.join(".git")` is a FILE, not a
    // directory, so this is where a shim baked at `--git-dir`-plus-"hooks"
    // would have actually landed: inert, since git never dispatches from here.
    assert!(
        !repo.join(".git/worktrees/feature/hooks").exists(),
        "baked into the worktree-private gitdir instead of the shared one"
    );
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

fn run_verb(s: &Sandbox, cwd: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", &s.0)
        .env("USERPROFILE", &s.0)
        .env("XDG_CONFIG_HOME", s.path(".config"))
        .output()
        .expect("run githooks");
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// The failure this guard was written for: `install` wrote all four dispatchers
/// unconditionally, so a `commit-msg` somebody had written themselves was
/// silently replaced. Same class as the two incidents that overwrote tracked
/// files, one directory along.
#[test]
fn install_refuses_to_overwrite_a_hook_it_did_not_write() {
    let s = Sandbox::new("foreign");
    let repo = s.path("repo");
    init_repo(&repo);
    let mine = repo.join(".git/hooks/commit-msg");
    std::fs::create_dir_all(repo.join(".git/hooks")).expect("mkdir");
    std::fs::write(&mine, "#!/bin/sh\necho MY OWN HOOK\n").expect("write");

    let (code, out) = s.install(&repo);
    assert_ne!(code, 0, "a silent overwrite reported success:\n{out}");
    assert!(out.contains("commit-msg"), "must name the file:\n{out}");
    assert_eq!(
        std::fs::read_to_string(&mine).expect("read"),
        "#!/bin/sh\necho MY OWN HOOK\n",
        "THEIR HOOK WAS OVERWRITTEN"
    );

    // …and `--force` is how you say you looked at it.
    let (code, out) = run_verb(&s, &repo, &["install", "--force"]);
    assert_eq!(code, 0, "{out}");
    assert!(std::fs::read_to_string(&mine)
        .expect("read")
        .contains("githooks"));
}

#[test]
fn uninstall_removes_our_shims_and_nothing_else() {
    let s = Sandbox::new("uninstall");
    let repo = s.path("repo");
    init_repo(&repo);
    assert_eq!(s.install(&repo).0, 0);
    // A hook of their own, added after we installed.
    let theirs = repo.join(".git/hooks/post-commit");
    std::fs::write(&theirs, "#!/bin/sh\necho theirs\n").expect("write");
    // And policy they set, which is a statement about their repo, not ours.
    git(&repo, &["config", "hook.skip", "pre-commit-clippy"]);

    let (code, out) = run_verb(&s, &repo, &["uninstall"]);
    assert_eq!(code, 0, "{out}");

    for name in ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"] {
        assert!(
            !repo.join(".git/hooks").join(name).exists(),
            "{name} survived uninstall:\n{out}"
        );
    }
    assert!(theirs.exists(), "a hook we did not write was deleted");
    let skips = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["config", "--get-all", "hook.skip"])
        .output()
        .expect("git");
    assert!(
        String::from_utf8_lossy(&skips.stdout).contains("pre-commit-clippy"),
        "uninstall discarded the user's own policy"
    );
    // The binary is shared with every other repository.
    installed_bin(&s);
}

/// Running it twice must not be an error — it is how somebody makes sure.
#[test]
fn uninstalling_twice_is_idempotent() {
    let s = Sandbox::new("uninstall-twice");
    let repo = s.path("repo");
    init_repo(&repo);
    assert_eq!(s.install(&repo).0, 0);
    assert_eq!(run_verb(&s, &repo, &["uninstall"]).0, 0);
    let (code, out) = run_verb(&s, &repo, &["uninstall"]);
    assert_eq!(code, 0, "second uninstall failed:\n{out}");
}

/// `--binary` is opt-in because other repositories are using it.
#[test]
fn uninstall_keeps_the_binary_unless_asked() {
    let s = Sandbox::new("uninstall-bin");
    let repo = s.path("repo");
    init_repo(&repo);
    assert_eq!(s.install(&repo).0, 0);
    let bin = installed_bin(&s);

    assert_eq!(run_verb(&s, &repo, &["uninstall"]).0, 0);
    assert!(bin.exists(), "uninstall took the shared binary unasked");

    assert_eq!(run_verb(&s, &repo, &["uninstall", "--binary"]).0, 0);
    assert!(!bin.exists(), "--binary did not remove it");
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

// ---- offer_agents_md ------------------------------------------------------

/// A test process has no controlling terminal, so `trust::confirm`'s
/// `/dev/tty` open fails and it answers "no" — the same path a real
/// non-interactive install takes. `install` must still succeed, and must not
/// have written anything nobody agreed to.
#[test]
fn a_non_interactive_install_completes_without_writing_agents_md() {
    let s = Sandbox::new("agents-md-noninteractive");
    let repo = s.path("repo");
    init_repo(&repo);

    let (code, out) = s.install(&repo);
    assert_eq!(code, 0, "{out}");
    assert!(
        !repo.join("AGENTS.md").exists(),
        "an unanswered prompt must not write anything"
    );
}

/// Re-running install after a declined (unanswered) prompt must stay
/// harmless — the same "twice is idempotent" property the shim install
/// already has.
#[test]
fn offering_agents_md_again_after_declining_is_harmless() {
    let s = Sandbox::new("agents-md-twice");
    let repo = s.path("repo");
    init_repo(&repo);

    assert_eq!(s.install(&repo).0, 0);
    let (code, out) = s.install(&repo);
    assert_eq!(code, 0, "a second install failed:\n{out}");
    assert!(!repo.join("AGENTS.md").exists());
}

// ---- what may be written over, and what may never be ----------------------

/// A compiled hook is not valid UTF-8, so `read_to_string` returned `Err`, and
/// `unwrap_or(false)` read that as "not foreign". `install` wrote a shim
/// straight over somebody's binary: no `--force`, no refusal, no message.
///
/// Asserted BYTE-identical rather than "still exists", because the failure
/// being guarded against is a successful overwrite, not a deletion.
#[test]
fn a_hook_it_cannot_decode_is_refused_and_left_byte_identical() {
    let s = Sandbox::new("binary-hook");
    let repo = s.path("repo");
    init_repo(&repo);
    std::fs::create_dir_all(repo.join(".git/hooks")).expect("mkdir");
    let theirs = repo.join(".git/hooks/pre-commit");
    // A little ELF header and some bytes no UTF-8 decoder accepts.
    let bytes: Vec<u8> = vec![
        0x7f, b'E', b'L', b'F', 0x02, 0x01, 0x01, 0x00, 0xff, 0xfe, 0x00,
    ];
    std::fs::write(&theirs, &bytes).expect("write");

    let (code, out) = s.install(&repo);
    assert_ne!(code, 0, "a silent overwrite reported success:\n{out}");
    assert!(
        out.contains("not valid UTF-8"),
        "the refusal must say what it could not read:\n{out}"
    );
    assert_eq!(
        std::fs::read(&theirs).expect("read"),
        bytes,
        "THEIR COMPILED HOOK WAS OVERWRITTEN"
    );
}

/// The verified bug, end to end: `.git/hooks/<name>` a symlink to a TRACKED
/// file in the working tree. `fs::write` follows the link, so every guard the
/// installer had — all of them about the link path, which is untracked and
/// inside `.git` and entirely unremarkable — was answering a question about a
/// different file than the one being written.
///
/// The property, stated for all four dispatchers at once: `--force` may change
/// the hook path, and may never change any file a hook path pointed at.
#[cfg(unix)]
#[test]
fn force_replaces_a_symlinked_hook_and_never_the_file_it_pointed_at() {
    let s = Sandbox::new("write-through");
    let repo = s.path("repo");
    init_repo(&repo);
    let dev = repo.join("devhooks");
    std::fs::create_dir_all(&dev).expect("mkdir");
    std::fs::create_dir_all(repo.join(".git/hooks")).expect("mkdir");

    let names = ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"];
    for name in names {
        let target = dev.join(name);
        std::fs::write(&target, format!("#!/bin/sh\n# PRECIOUS {name}\n")).expect("write");
        std::os::unix::fs::symlink(&target, repo.join(".git/hooks").join(name)).expect("symlink");
    }
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "seed"]);
    let before: Vec<String> = names
        .iter()
        .map(|n| std::fs::read_to_string(dev.join(n)).expect("read"))
        .collect();

    // Plain install refuses, and names the link rather than muttering about
    // hooks that are "not ours".
    let (code, out) = s.install(&repo);
    assert_ne!(
        code, 0,
        "a write through a symlink reported success:\n{out}"
    );
    assert!(out.contains("symlink"), "the refusal must name it:\n{out}");

    // …and `--force` replaces the LINK.
    let (code, out) = run_verb(&s, &repo, &["install", "--force"]);
    assert_eq!(code, 0, "{out}");
    for (i, name) in names.iter().enumerate() {
        assert_eq!(
            std::fs::read_to_string(dev.join(name)).expect("read"),
            before[i],
            "THE WRITE WENT THROUGH THE LINK for {name}"
        );
        let hook = repo.join(".git/hooks").join(name);
        assert!(
            !std::fs::symlink_metadata(&hook)
                .expect("stat")
                .file_type()
                .is_symlink(),
            "{name} is still a symlink after --force"
        );
        assert!(
            std::fs::read_to_string(&hook)
                .expect("read")
                .contains("githooks"),
            "{name} was not replaced with a shim"
        );
    }
}

/// `--force` is a statement about `.git/hooks`. It is not consent to rewrite a
/// file git is watching, and there is no flag that is — a repository pointing
/// `core.hooksPath` at a tracked directory (a perfectly normal way to keep
/// hooks in version control) must be refused whatever is typed.
#[test]
fn a_tracked_hooks_directory_is_refused_even_with_force() {
    let s = Sandbox::new("tracked-hooks");
    let repo = s.path("repo");
    init_repo(&repo);
    let dev = repo.join("devhooks");
    std::fs::create_dir_all(&dev).expect("mkdir");
    for name in ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"] {
        std::fs::write(dev.join(name), format!("#!/bin/sh\n# tracked {name}\n")).expect("write");
    }
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "seed"]);
    git(&repo, &["config", "core.hooksPath", "devhooks"]);
    let before = std::fs::read_to_string(dev.join("pre-commit")).expect("read");

    for args in [&["install"][..], &["install", "--force"][..]] {
        let (code, out) = run_verb(&s, &repo, args);
        assert_ne!(code, 0, "{args:?} rewrote tracked source:\n{out}");
        assert!(
            out.contains("TRACKED") || out.contains("not ours"),
            "{args:?} must say why:\n{out}"
        );
        assert_eq!(
            std::fs::read_to_string(dev.join("pre-commit")).expect("read"),
            before,
            "TRACKED FILE WAS REWRITTEN by {args:?}"
        );
    }
}

/// Fail closed for the WHOLE repository. A write that cannot happen must leave
/// zero dispatchers, not two — the state in which a repo runs half its checks
/// and nothing says so.
#[cfg(unix)]
#[test]
fn a_write_that_cannot_happen_leaves_no_dispatchers_at_all() {
    use std::os::unix::fs::PermissionsExt;
    // root ignores the permission bits and the test would prove nothing.
    if unsafe { libc_geteuid() } == 0 {
        return;
    }
    let s = Sandbox::new("halfwritten");
    let repo = s.path("repo");
    init_repo(&repo);
    let hooks = repo.join(".git/hooks");
    std::fs::create_dir_all(&hooks).expect("mkdir");
    std::fs::set_permissions(&hooks, std::fs::Permissions::from_mode(0o555)).expect("chmod");

    let (code, out) = s.install(&repo);
    let _ = std::fs::set_permissions(&hooks, std::fs::Permissions::from_mode(0o755));

    assert_ne!(code, 0, "a failed write reported success:\n{out}");
    for name in ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"] {
        assert!(
            !hooks.join(name).exists(),
            "{name} landed despite the install failing:\n{out}"
        );
    }
    // And no staging litter either.
    let leftovers: Vec<_> = std::fs::read_dir(&hooks)
        .expect("readdir")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".githooks-tmp-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "staging litter left behind: {leftovers:?}"
    );
}

/// `--force` printed "baked 4 shims" and nothing else. The user typed it
/// because a file was in the way; the one thing the output never said was which
/// file, or what it was. `.git` is not tracked, so there is nothing to check it
/// back out from — the message is the only record that will ever exist.
#[test]
fn force_names_what_it_overwrote() {
    let s = Sandbox::new("force-names");
    let repo = s.path("repo");
    init_repo(&repo);
    std::fs::create_dir_all(repo.join(".git/hooks")).expect("mkdir");
    std::fs::write(
        repo.join(".git/hooks/commit-msg"),
        "#!/bin/sh\necho MY OWN HOOK\n",
    )
    .expect("write");

    let (code, out) = run_verb(&s, &repo, &["install", "--force"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("overwrote") && out.contains("commit-msg"),
        "--force must say what it took:\n{out}"
    );
    // And it must not claim to have overwritten the three that were absent.
    assert_eq!(
        out.matches("overwrote").count(),
        1,
        "--force reported overwrites that did not happen:\n{out}"
    );
}

/// The README promises a foreign hook is "left alone and named". It was true
/// only for hooks that happened to be valid UTF-8: the loop matched
/// `Err(_) => {}` on `read_to_string`, so a compiled hook was passed over in
/// total silence — not removed, not counted, not mentioned.
#[test]
fn uninstall_names_a_hook_it_could_not_read() {
    let s = Sandbox::new("uninstall-binary");
    let repo = s.path("repo");
    init_repo(&repo);
    assert_eq!(s.install(&repo).0, 0);
    let theirs = repo.join(".git/hooks/commit-msg");
    std::fs::write(&theirs, [0x7f, b'E', b'L', b'F', 0xff, 0xfe]).expect("write");

    let (code, out) = run_verb(&s, &repo, &["uninstall"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("commit-msg") && out.contains("not valid UTF-8"),
        "an unreadable hook was passed over in silence:\n{out}"
    );
    assert!(theirs.exists(), "an unreadable hook was deleted");
}

// ---- uninstall undoes install ---------------------------------------------

/// `uninstall` did not undo `install`. It removed the repository's shims and
/// left the template directory populated, so with `init.templateDir` set every
/// subsequent `git clone` re-installed the hooks into the new repo — after the
/// user had been told the uninstall succeeded.
#[test]
fn uninstall_takes_back_the_template_dir_and_names_the_standing_grant() {
    let s = Sandbox::new("uninstall-template");
    let repo = s.path("repo");
    init_repo(&repo);
    assert_eq!(s.install(&repo).0, 0);

    let tpl = s.path(".config/git/git-templates/templates/hooks");
    for name in ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"] {
        assert!(tpl.join(name).is_file(), "install wrote no template shims");
    }
    // The standing grant itself, set the way the README tells people to.
    let cfg = Command::new("git")
        .args(["config", "--global", "init.templateDir"])
        .arg(s.path(".config/git/git-templates"))
        .env("HOME", &s.0)
        .env("USERPROFILE", &s.0)
        .env("XDG_CONFIG_HOME", s.path(".config"))
        .output()
        .expect("git config");
    assert!(cfg.status.success());

    let (code, out) = run_verb(&s, &repo, &["uninstall"]);
    assert_eq!(code, 0, "{out}");
    for name in ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"] {
        assert!(
            !tpl.join(name).exists(),
            "{name} survived in the template dir — the next clone reinstalls it:\n{out}"
        );
    }
    assert!(
        out.contains("init.templateDir"),
        "uninstall must name the setting that keeps installing hooks:\n{out}"
    );
    assert!(
        out.contains("git config --global --unset init.templateDir"),
        "and give the command that revokes it:\n{out}"
    );
}

/// The other half, and the dangerous one: when the template dir is a CHECKOUT
/// reached through a symlink, those shims are tracked source. `install` refuses
/// to write there; `uninstall` must refuse to delete there, and a delete has no
/// `--force` to argue with.
#[cfg(unix)]
#[test]
fn uninstall_refuses_a_template_dir_that_is_a_checkout() {
    let s = Sandbox::new("uninstall-checkout");
    let checkout = s.path("checkout");
    init_repo(&checkout);
    let hooks = checkout.join("templates/hooks");
    std::fs::create_dir_all(&hooks).expect("mkdir");
    for name in ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"] {
        std::fs::write(
            hooks.join(name),
            "#!/bin/sh\n# git-templates hook shim.\nexec githooks __GITHOOKS_BIN__\n",
        )
        .expect("write");
    }
    git(&checkout, &["add", "-A"]);
    git(&checkout, &["commit", "-qm", "seed"]);

    let cfg = s.path(".config/git");
    std::fs::create_dir_all(&cfg).expect("mkdir");
    std::os::unix::fs::symlink(&checkout, cfg.join("git-templates")).expect("symlink");

    let repo = s.path("repo");
    init_repo(&repo);
    let (code, out) = run_verb(&s, &repo, &["uninstall"]);

    assert_eq!(code, 0, "refusing is not an error:\n{out}");
    assert!(
        out.contains("checkout") && out.contains("NOTHING"),
        "the refusal must be explained:\n{out}"
    );
    assert!(
        out.contains(
            &checkout
                .canonicalize()
                .expect("canon")
                .display()
                .to_string()
        ),
        "and must name the RESOLVED path, not the symlink:\n{out}"
    );
    for name in ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"] {
        assert!(
            hooks.join(name).is_file(),
            "TRACKED FILE {name} WAS DELETED:\n{out}"
        );
    }
}

/// Once the block already matches what would be generated, `offer_agents_md`
/// must skip silently — no prompt at all — the same way `offer_trust` skips
/// once a manifest is already trusted.
#[test]
fn an_up_to_date_agents_md_is_not_re_offered() {
    let s = Sandbox::new("agents-md-uptodate");
    let repo = s.path("repo");
    init_repo(&repo);

    let write = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("agents-md")
        .current_dir(&repo)
        .output()
        .expect("run githooks agents-md");
    assert!(write.status.success());

    let (code, out) = s.install(&repo);
    assert_eq!(code, 0, "{out}");
    assert!(
        !out.contains("AGENTS.md can point"),
        "an already up-to-date block must not be offered again:\n{out}"
    );
}

/// Run the installer with a custom `$GITHOOKS_BIN_DIR`, the way somebody who
/// keeps their tools outside `~/.local/bin` would.
fn install_with_bin_dir(s: &Sandbox, cwd: &Path, bin_dir: &Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("install")
        .current_dir(cwd)
        .env("HOME", &s.0)
        .env("USERPROFILE", &s.0)
        .env("XDG_CONFIG_HOME", s.path(".config"))
        .env("GITHOOKS_BIN_DIR", bin_dir)
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

/// A template dir that is a checkout, with the binary somewhere else.
///
/// Both halves are supported and they stop composing together: the checkout's
/// shims keep the placeholder on purpose and resolve at run time from
/// `$HOME/.local/bin`, which the shim hardcodes — so a binary installed
/// elsewhere is one the shims will never find.
#[cfg(unix)]
fn checkout_template_dir(s: &Sandbox) -> Option<std::path::PathBuf> {
    let checkout = s.path("checkout");
    init_repo(&checkout);
    let hooks = checkout.join("templates/hooks");
    std::fs::create_dir_all(&hooks).expect("mkdir");
    std::fs::write(hooks.join("pre-commit"), "precious __GITHOOKS_BIN__\n").expect("write");
    git(&checkout, &["add", "-A"]);
    git(&checkout, &["commit", "-qm", "seed"]);

    let cfg = s.path(".config/git");
    std::fs::create_dir_all(&cfg).expect("mkdir");
    std::os::unix::fs::symlink(&checkout, cfg.join("git-templates")).ok()?;
    Some(checkout)
}

/// The combination is flagged where it is created, not at somebody's next
/// commit in a repository they have not thought about since.
#[cfg(unix)]
#[test]
fn a_binary_an_unbaked_shim_cannot_find_is_named() {
    let s = Sandbox::new("binhint");
    if checkout_template_dir(&s).is_none() {
        println!("  ! symlinks unavailable — skipping");
        return;
    }
    let repo = s.path("repo");
    init_repo(&repo);
    let elsewhere = s.path("opt/tools");
    std::fs::create_dir_all(&elsewhere).expect("mkdir");

    let (code, out) = install_with_bin_dir(&s, &repo, &elsewhere);

    assert_eq!(code, 0, "the install itself should still succeed:\n{out}");
    assert!(
        out.contains("unbaked shim will not find"),
        "the unreachable binary was not flagged:\n{out}"
    );
    // Both halves of the fix, and both ways out.
    assert!(
        out.contains("opt/tools"),
        "did not name where it went:\n{out}"
    );
    assert!(
        out.contains(".local/bin"),
        "did not name where shims look:\n{out}"
    );
    assert!(out.contains("ln -s"), "offered no link:\n{out}");
    assert!(
        out.contains("GIT_HOOKS_BIN"),
        "did not name the runtime override:\n{out}"
    );
}

/// …and it must stay quiet for everybody else, or it is just noise on every
/// install. The default bin dir IS the directory the shims look in.
#[cfg(unix)]
#[test]
fn the_default_bin_dir_is_not_flagged() {
    let s = Sandbox::new("binhint-default");
    if checkout_template_dir(&s).is_none() {
        println!("  ! symlinks unavailable — skipping");
        return;
    }
    let repo = s.path("repo");
    init_repo(&repo);

    // No GITHOOKS_BIN_DIR: the binary lands in $HOME/.local/bin, where an
    // unbaked shim looks. `HOME` is the sandbox, so this is self-contained.
    let (code, out) = s.install(&repo);

    assert_eq!(code, 0, "{out}");
    assert!(
        !out.contains("unbaked shim will not find"),
        "warned about the default layout, which is the whole fleet:\n{out}"
    );
}

/// Run the installer with a chosen `PATH`, and with `$GITHOOKS_BIN_DIR`
/// deliberately UNSET — the package-manager case.
fn install_on_path(s: &Sandbox, cwd: &Path, path: &str) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_githooks"))
        .arg("install")
        .current_dir(cwd)
        .env("HOME", &s.0)
        .env("USERPROFILE", &s.0)
        .env("XDG_CONFIG_HOME", s.path(".config"))
        .env_remove("GITHOOKS_BIN_DIR")
        .env("PATH", path)
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

/// The baked path, read back out of a shim this install wrote.
fn baked_in(repo: &Path) -> String {
    let shim = std::fs::read_to_string(repo.join(".git/hooks/pre-commit")).expect("read shim");
    shim.lines()
        .find_map(|l| l.trim().strip_prefix("BAKED=\""))
        .and_then(|l| l.strip_suffix('"'))
        .map(str::to_string)
        .unwrap_or_else(|| panic!("no baked path in the shim:\n{shim}"))
}

/// A binary a package manager already put on PATH is baked where it is.
///
/// Copying it would produce a second, unmanaged copy that `brew upgrade` never
/// touches again — the machine this was written on was in exactly that state.
#[test]
fn a_binary_already_on_path_is_not_copied() {
    let s = Sandbox::new("onpath");
    let repo = s.path("repo");
    init_repo(&repo);

    // Stand the binary somewhere that IS on PATH, the way a package would.
    let pkg = s.path("pkg/bin");
    std::fs::create_dir_all(&pkg).expect("mkdir");
    let placed = pkg.join(if cfg!(windows) {
        "githooks.exe"
    } else {
        "githooks"
    });
    std::fs::copy(env!("CARGO_BIN_EXE_githooks"), &placed).expect("copy");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&placed, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    let (code, out) = run_from(&s, &placed, &repo, &pkg.to_string_lossy());
    assert_eq!(code, 0, "{out}");

    let baked = baked_in(&repo);
    assert_eq!(
        std::path::Path::new(&baked).canonicalize().ok(),
        placed.canonicalize().ok(),
        "did not bake the binary where it already was (baked {baked}):\n{out}"
    );
    assert!(
        !s.path(".local/bin/githooks").exists(),
        "made a second copy the package manager will never update:\n{out}"
    );
    assert!(
        out.contains("nothing was copied"),
        "did not say it skipped the copy:\n{out}"
    );
}

/// The Homebrew shape: `bin/githooks` is a symlink into a VERSIONED directory.
///
/// The symlink is the stable name; its target is deleted on the next upgrade.
/// Baking the resolved path would pin every repository to a version that is
/// about to stop existing — worse than the copy this avoids.
#[cfg(unix)]
#[test]
fn a_symlink_on_path_bakes_the_link_not_its_versioned_target() {
    let s = Sandbox::new("brewshape");
    let repo = s.path("repo");
    init_repo(&repo);

    let cellar = s.path("cellar/1.0.0/bin");
    let bin = s.path("brew/bin");
    std::fs::create_dir_all(&cellar).expect("mkdir");
    std::fs::create_dir_all(&bin).expect("mkdir");
    let real = cellar.join("githooks");
    std::fs::copy(env!("CARGO_BIN_EXE_githooks"), &real).expect("copy");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let link = bin.join("githooks");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");

    let (code, out) = run_from(&s, &link, &repo, &bin.to_string_lossy());
    assert_eq!(code, 0, "{out}");

    let baked = baked_in(&repo);
    assert!(
        !baked.contains("/1.0.0/"),
        "baked the VERSIONED path, which the next upgrade deletes: {baked}"
    );
    assert!(
        baked.ends_with("brew/bin/githooks"),
        "expected the PATH entry, got {baked}"
    );
}

/// The build-directory case, which is why the copy exists at all: nothing on
/// PATH, so it must still be copied somewhere `cargo clean` will not delete.
#[test]
fn a_binary_not_on_path_is_still_copied() {
    let s = Sandbox::new("notonpath");
    let repo = s.path("repo");
    init_repo(&repo);

    // A PATH with no githooks in it — git still has to be reachable.
    // The inherited PATH: it has git, and identity matching means an
    // unrelated githooks on it is not mistaken for the one under test.
    let inherited = std::env::var("PATH").unwrap_or_default();
    let (code, out) = install_on_path(&s, &repo, &inherited);
    assert_eq!(code, 0, "{out}");

    let copied = s.path(".local/bin/githooks");
    assert!(
        copied.exists() || s.path(".local/bin/githooks.exe").exists(),
        "a binary that is not on PATH must still be copied somewhere stable:\n{out}"
    );
}

/// Setting the variable IS the request to put it somewhere specific.
#[test]
fn githooks_bin_dir_still_forces_a_copy() {
    let s = Sandbox::new("forcedcopy");
    let repo = s.path("repo");
    init_repo(&repo);
    let elsewhere = s.path("opt/bin");
    std::fs::create_dir_all(&elsewhere).expect("mkdir");

    let (code, out) = install_with_bin_dir(&s, &repo, &elsewhere);
    assert_eq!(code, 0, "{out}");
    assert!(
        elsewhere.join("githooks").exists() || elsewhere.join("githooks.exe").exists(),
        "GITHOOKS_BIN_DIR was ignored:\n{out}"
    );
}

/// Invoke a specific copy of the binary, with `dir` on PATH.
fn run_from(s: &Sandbox, exe: &Path, cwd: &Path, dir: &str) -> (i32, String) {
    let out = Command::new(exe)
        .arg("install")
        .current_dir(cwd)
        .env("HOME", &s.0)
        .env("USERPROFILE", &s.0)
        .env("XDG_CONFIG_HOME", s.path(".config"))
        .env_remove("GITHOOKS_BIN_DIR")
        // git must stay reachable, so this PREPENDS to the inherited PATH
        // rather than replacing it — and joins with `env::join_paths`, because
        // the separator is `;` on Windows and a hand-built `a:b` string leaves
        // that runner with no git at all. Matching is by file IDENTITY, not by
        // name, so an unrelated githooks already on PATH cannot be mistaken
        // for this one.
        .env("PATH", path_with(dir))
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

/// A build directory on PATH must never be baked, even though it IS on PATH.
///
/// cargo prepends its build directory to PATH when running tests on Windows,
/// so this is the state the suite itself runs in there — and a shim baked
/// against `target/debug` stops resolving the moment anybody rebuilds.
#[test]
fn a_build_directory_on_path_is_not_baked() {
    let s = Sandbox::new("builddir");
    let repo = s.path("repo");
    init_repo(&repo);

    // A directory carrying cargo's own marker for "this is build output".
    let build = s.path("proj/target/debug");
    std::fs::create_dir_all(&build).expect("mkdir");
    std::fs::write(
        s.path("proj/target/CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )
    .expect("write tag");
    let placed = build.join(if cfg!(windows) {
        "githooks.exe"
    } else {
        "githooks"
    });
    std::fs::copy(env!("CARGO_BIN_EXE_githooks"), &placed).expect("copy");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&placed, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    let (code, out) = run_from(&s, &placed, &repo, &build.to_string_lossy());
    assert_eq!(code, 0, "{out}");

    let baked = baked_in(&repo);
    assert!(
        !baked.contains("target"),
        "baked a build directory, which the next rebuild empties: {baked}"
    );
    assert!(
        s.path(".local/bin/githooks").exists() || s.path(".local/bin/githooks.exe").exists(),
        "and did not copy it somewhere durable instead:\n{out}"
    );
}

/// `dir` prepended to the inherited PATH, joined the way this platform joins
/// paths. A hand-built `"a:b"` is wrong on Windows and leaves the runner
/// without git.
fn path_with(dir: &str) -> std::ffi::OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs = vec![std::path::PathBuf::from(dir)];
    dirs.extend(std::env::split_paths(&inherited));
    std::env::join_paths(dirs).expect("join PATH")
}
