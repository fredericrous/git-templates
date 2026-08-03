//! `githooks install` — put the binary somewhere stable and wire up the shims.
//!
//! This was a Makefile recipe. It moved here for one reason: the guard below
//! decides whether a directory is safe to delete, it has been got wrong TWICE —
//! both times overwriting tracked source files with machine-specific paths — and
//! shell that runs on one platform cannot be tested on three.
//!
//! Everything here is `std`. The commit path's dependency posture
//! (`scripts/check-no-deps.sh`) is unchanged: this adds code, not crates.
//!
//! ## Why it is a subcommand and not a script
//!
//! A `.ps1` for Windows plus a Makefile for Unix would be two implementations of
//! that guard, in two languages, one of them untested — for a routine whose
//! failure mode is deleting your work. And `make` is not the Unix-only detail it
//! looks like: Git for Windows ships `bash`, `sh` and coreutils but NOT `make`,
//! so the dependency was the problem rather than the shell.
//!
//! The shim text is embedded with `include_str!`, so an installed binary carries
//! its own shims and can install from any directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ui::{highlight, valid_sign, warning_sign};

/// The token every shim carries until it is baked.
pub const PLACEHOLDER: &str = "__GITHOOKS_BIN__";

/// The one shim. All four git-invoked hooks are the same file — it passes its
/// own filename through — and `shims_on_disk_match_the_embedded_one` keeps this
/// copy honest against `templates/hooks/`.
pub const SHIM: &str = include_str!("../../../templates/hooks/pre-commit");

/// The ownership question, and every other "may we touch this file?" answer,
/// now live in [`crate::hookfile`] — one implementation that fails closed,
/// rather than the three `unwrap_or(false)` one-liners that used to answer it
/// here, in `fleet::scan` and in `fleet::fix`.
///
/// Re-exported rather than moved outright because both fleet call sites and the
/// dashboard's `shim` module name them through this path, and a rename would be
/// churn in files this change has no business editing.
pub use crate::hookfile::{is_our_shim, SHIM_MARKER};

/// The hook names git actually invokes, and so the only files we install.
pub const DISPATCHERS: [&str; 4] = ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"];

/// What may be done with a candidate template directory.
///
/// `~/.config/git/git-templates` is commonly a SYMLINK to a checkout of this
/// repository, in which case "installing" there means deleting and overwriting
/// TRACKED files.
///
/// Comparing the path against the source tree is NOT enough, and that is the
/// mistake that caused both incidents: run the install from a git worktree and
/// the two resolve to different paths — a different checkout of the same repo —
/// so a path comparison says "not the source" and clobbers the main checkout.
/// Asking git is the reliable test whatever route the symlink took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateDir {
    /// The path does not exist and could not be created.
    Unresolvable,
    /// No git to ask. Refuse rather than guess.
    NoGit,
    /// It holds tracked files: it IS a checkout. Nothing to install — `git init`
    /// already reads its templates from there, and the shims keep their
    /// placeholder and resolve the binary at run time.
    IsCheckout,
    /// Inside a checkout but tracking nothing here. Still not ours to empty.
    InsideCheckout,
    /// An ordinary directory. Safe to populate.
    Safe,
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Decide what may be done with `dir`. Never mutates anything.
pub fn classify_dir(dir: &Path) -> TemplateDir {
    let Ok(real) = dir.canonicalize() else {
        return TemplateDir::Unresolvable;
    };
    if Command::new("git").arg("--version").output().is_err() {
        return TemplateDir::NoGit;
    }
    // `ls-files --error-unmatch .` is the question that matters: does git track
    // anything HERE? A directory can be inside a checkout and still be
    // untracked scratch space, which the next test separates.
    if git_ok(&real, &["ls-files", "--error-unmatch", "."]) {
        return TemplateDir::IsCheckout;
    }
    if git_ok(&real, &["rev-parse", "--git-dir"]) {
        return TemplateDir::InsideCheckout;
    }
    TemplateDir::Safe
}

/// Write the absolute binary path into a shim.
///
/// A plain global replace, which is why the shim's own comment must not spell
/// the token out — it did, and every baked shim carried an "explanation" whose
/// text was a machine path. Idempotent: re-baking a baked shim is a no-op
/// because the token is gone.
pub fn bake(shim: &str, bin: &str) -> String {
    shim.replace(PLACEHOLDER, bin)
}

/// Whether the shim will accept `bin` as its baked path.
///
/// The shim rejects anything relative before it ever touches the filesystem,
/// and this is the same rule stated where the value is produced. Git runs a
/// hook with the working tree as the current directory, so a relative baked
/// path is a question asked of the REPOSITORY — and a clone that ships a file
/// by that name gets to answer it. Baking one would install a hook that either
/// cannot resolve its binary or resolves it to somebody else's, so refuse here
/// too, where the message can say which path was wrong.
///
/// Absolute means POSIX `/…` or a Windows drive path (`C:\…`, `C:/…`).
pub fn is_bakeable(bin: &str) -> bool {
    if bin.is_empty() || bin == PLACEHOLDER {
        return false;
    }
    let b = bin.as_bytes();
    if b[0] == b'/' {
        return true;
    }
    b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\')
}

/// An absolute form of `p`, for baking.
///
/// NOT `canonicalize`: that returns an extended-length path (`\\?\C:\…`) on
/// Windows, which `sh` cannot test, and it fails outright on a path that does
/// not exist yet. `$GITHOOKS_BIN_DIR` may be relative, which is the route by
/// which a relative path could reach a shim at all.
fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        return p.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(p),
        Err(_) => p.to_path_buf(),
    }
}

/// `~/.local/bin`, or `$GITHOOKS_BIN_DIR`.
pub fn bin_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("GITHOOKS_BIN_DIR") {
        return PathBuf::from(d);
    }
    home().join(".local").join("bin")
}

/// `$XDG_CONFIG_HOME/git/git-templates/templates/hooks`.
pub fn template_hooks_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    base.join("git")
        .join("git-templates")
        .join("templates")
        .join("hooks")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The name to install under. Windows builds `githooks.exe`, and a shim testing
/// `[ -x .../githooks ]` is false for it.
fn installed_name() -> String {
    match std::env::current_exe() {
        Ok(p) => name_for(&p),
        Err(_) => "githooks".to_string(),
    }
}

/// Split from `installed_name` so it can be tested on every platform rather
/// than only the one that produces a `.exe`. A `cfg!(windows)` assertion is
/// vacuous on the machine most of this is written on.
fn name_for(exe: &Path) -> String {
    match exe.extension().and_then(|e| e.to_str()) {
        Some(e) if !e.is_empty() => format!("githooks.{e}"),
        _ => "githooks".to_string(),
    }
}

/// Hook files in `dir` that exist and are NOT ours.
///
/// `install` used to write all four unconditionally, which silently destroyed a
/// `commit-msg` somebody had written themselves. That is the same failure as the
/// two that overwrote tracked files, one directory along, and it had no guard at
/// all — the fleet's `fix` planner has one and the per-repo installer never did.
fn foreign_hooks(dir: &Path) -> Vec<&'static str> {
    DISPATCHERS
        .into_iter()
        .filter(|name| {
            std::fs::read_to_string(dir.join(name))
                .map(|text| !is_our_shim(&text))
                .unwrap_or(false)
        })
        .collect()
}

fn write_shims(dir: &Path, bin: &str) -> std::io::Result<usize> {
    // Fail closed rather than write four hooks that resolve to nothing — or, if
    // the repository happens to hold a file by that name, to something.
    if !is_bakeable(bin) {
        return Err(std::io::Error::other(format!(
            "refusing to bake {bin:?}: the shim takes an absolute path only"
        )));
    }
    let baked = bake(SHIM, bin);
    let mut n = 0;
    for name in DISPATCHERS {
        let path = dir.join(name);
        std::fs::write(&path, &baked)?;
        make_executable(&path)?;
        n += 1;
    }
    Ok(n)
}

#[cfg(unix)]
fn make_executable(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_p: &Path) -> std::io::Result<()> {
    Ok(()) // Windows has no execute bit; git runs the shim through sh regardless.
}

/// Install: copy this binary somewhere stable, populate the template directory
/// if that is safe, and bake the current repository's hooks.
///
/// Three steps, three functions. This was one 88-line body whose own comments
/// numbered its sections — which is the tell that the sections wanted to be
/// functions.
pub fn run(force: bool) -> Result<(), String> {
    let binary = install_binary()?;
    populate_template_dir(&binary)?;
    bake_repo_hooks(&binary, force)?;
    offer_trust();
    offer_agents_md();
    Ok(())
}

/// Ask about the manifest, once, at the moment somebody is already deciding
/// about this repository.
///
/// `direnv` has to ask lazily on `cd` because it has no install step to hang
/// the question from. We have one — so this is a single question, shown with
/// the declarations in view, and declining still leaves the built-ins working.
///
/// Never blocks and never fails the install: a repository that declares nothing
/// says nothing, and a non-interactive install simply reports the state.
fn offer_trust() {
    let root = crate::hooks::common::repo_root();
    let root = Path::new(&root);
    let state = crate::trust::state(root);
    if matches!(
        state,
        crate::trust::State::NoManifest | crate::trust::State::Trusted
    ) {
        return;
    }

    println!();
    println!(
        "{} {} declares checks that would run on your commits:",
        warning_sign(),
        crate::manifest::MANIFEST
    );
    // Read ONCE, then show and fingerprint that same buffer. `confirm()` blocks
    // on a keypress, sometimes for several seconds, and a file rewritten in
    // that window must not be trusted under the guise of the content that was
    // displayed — `record_verified` re-checks this fingerprint once the answer
    // is in. Reading separately to show and to hash would leave the same gap
    // one step earlier: the listing approved need not be the one recorded.
    let manifest = root.join(crate::manifest::MANIFEST);
    let Ok(source) = std::fs::read(&manifest) else {
        println!(
            "{} could not read {}",
            warning_sign(),
            crate::manifest::MANIFEST
        );
        return;
    };
    print!(
        "{}",
        crate::trust::describe_source(&String::from_utf8_lossy(&source))
    );
    let Some(fp) = crate::trust::fingerprint_bytes(root, &source) else {
        println!(
            "{} could not hash {}",
            warning_sign(),
            crate::manifest::MANIFEST
        );
        return;
    };
    if crate::trust::confirm("    Trust them? (y/N) ") {
        match crate::trust::record_verified(root, &fp) {
            Ok(()) => println!("{} trusted ({fp})", valid_sign()),
            Err(e) => println!("{} {e}", warning_sign()),
        }
    } else {
        println!("    Left untrusted. The built-ins still run; these do not.");
        println!("    Change your mind with `githooks trust`.");
    }
}

/// Ask about `AGENTS.md`, once, right where `offer_trust` asks about the
/// manifest — same reasoning, same shape: a single question with an install
/// step to hang it from, and declining changes nothing about how the hooks
/// themselves run.
///
/// This is the first thing `install` would write to TRACKED repo content —
/// everything else here lives in `.git/hooks` (never tracked) or a
/// machine-local path (`~/.local/bin`, the XDG template dir). That is exactly
/// why it is a confirm, not a silent write: `crate::agents_md::write` is
/// marker-scoped and safe to re-run, but "safe to overwrite" is not the same
/// promise as "yours to write unasked."
///
/// Never blocks and never fails the install: skips silently when there is
/// nothing to offer, and a non-interactive install simply leaves the
/// question unanswered — `trust::confirm` already treats no tty as "no".
fn offer_agents_md() {
    let root = crate::hooks::common::repo_root();
    let path = Path::new(&root).join("AGENTS.md");
    match crate::agents_md::check(&path) {
        Ok(crate::agents_md::CheckResult::MatchesGenerated) => return,
        Ok(_) => {}
        // Malformed markers: nothing this prompt can safely offer to fix.
        Err(_) => return,
    }

    println!();
    println!(
        "{} AGENTS.md can point coding agents at `githooks list --json` \
         instead of leaving them to discover these checks the hard way:",
        warning_sign()
    );
    if crate::trust::confirm("    Add it? (y/N) ") {
        match crate::agents_md::write(&path) {
            Ok(()) => println!("{} wrote {}", valid_sign(), path.display()),
            Err(e) => println!("{} {e}", warning_sign()),
        }
    } else {
        println!("    Left as-is. Change your mind with `githooks agents-md`.");
    }
}

/// Copy the running binary to a stable location, and return where it now lives.
fn install_binary() -> Result<String, String> {
    let me =
        std::env::current_exe().map_err(|e| format!("cannot locate the running binary: {e}"))?;
    let dir = bin_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

    // Absolute from here on: this path is what gets baked into every shim, and
    // `bin_dir()` honours `$GITHOOKS_BIN_DIR`, which may be relative.
    let target = absolute(&dir.join(installed_name()));
    // Copying a running binary over ITSELF fails on some platforms and is
    // pointless on all of them.
    let already_there = me.canonicalize().ok() == target.canonicalize().ok();
    if !already_there {
        std::fs::copy(&me, &target)
            .map_err(|e| format!("cannot install to {}: {e}", target.display()))?;
        make_executable(&target).map_err(|e| format!("cannot chmod {}: {e}", target.display()))?;
    }
    let installed = target.to_string_lossy().into_owned();
    println!("{} installed {}", valid_sign(), highlight(&installed));
    Ok(installed)
}

/// Write the shims into the template directory — unless doing so would delete
/// somebody's source.
///
/// REFUSING is not an error: on a machine where the template dir is the
/// checkout, there is nothing to install and the install has succeeded. FAILING
/// to write one it was allowed to write is, though — reporting success after a
/// step did not happen is the thing this whole codebase is arranged against.
fn populate_template_dir(binary: &str) -> Result<(), String> {
    let dir = template_hooks_dir();
    let _ = std::fs::create_dir_all(&dir);
    // Report the RESOLVED path. "It is the checkout" is only useful with the
    // checkout named, and the configured path is usually the symlink that hides
    // exactly that.
    let shown = dir.canonicalize().unwrap_or_else(|_| dir.clone());
    let shown = shown.display();

    match classify_dir(&dir) {
        TemplateDir::IsCheckout => {
            println!(
                "{} template dir IS the checkout ({shown}) — nothing to install.",
                warning_sign()
            );
            println!("    Its shims keep the placeholder deliberately and resolve");
            println!("    {binary} at run time. This is the intended setup.");
        }
        TemplateDir::InsideCheckout => println!(
            "{} {shown} is inside a git checkout — leaving it alone.",
            warning_sign()
        ),
        TemplateDir::NoGit => println!(
            "{} git is not on PATH — refusing to delete anything.",
            warning_sign()
        ),
        TemplateDir::Unresolvable => {
            println!("{} cannot resolve {shown} — skipping.", warning_sign())
        }
        TemplateDir::Safe => {
            let written = write_shims(&dir, binary)
                .map_err(|e| format!("cannot write shims to {shown}: {e}"))?;
            println!("{} wrote {written} shims to {shown}", valid_sign());
        }
    }
    Ok(())
}

/// Where git will actually look for hooks in the repository we are standing
/// in — never `--git-dir` plus `join("hooks")`. For the main worktree the two
/// agree, but hooks are explicitly SHARED across every worktree, unlike
/// `MERGE_HEAD` and friends: a linked worktree's `--git-dir` is its own
/// PRIVATE gitdir, so joining "hooks" onto it names a directory git never
/// dispatches from, and a shim baked there is inert — installed, and never
/// run. `--git-path hooks` is the question actually being asked, and git
/// itself resolves the worktree case correctly.
fn repo_hooks_dir() -> Option<PathBuf> {
    crate::git::stdout(&["rev-parse", "--path-format=absolute", "--git-path", "hooks"])
        .map(PathBuf::from)
}

/// Bake the shims into the repository we are standing in, if we are in one.
fn bake_repo_hooks(binary: &str, force: bool) -> Result<(), String> {
    let Some(hooks) = repo_hooks_dir() else {
        println!(
            "{} not inside a git repository — no repo hooks written.",
            warning_sign()
        );
        return Ok(());
    };
    let _ = std::fs::create_dir_all(&hooks);

    // Fail closed, and for the whole repository rather than per file: a partial
    // install is how a repo ends up with two of four hooks and no way to tell.
    let foreign = foreign_hooks(&hooks);
    if !foreign.is_empty() && !force {
        return Err(format!(
            "{} {} already has hooks that are not ours: {}\n    Look at them first, then `githooks install --force`.",
            crate::ui::error_sign(),
            hooks.display(),
            foreign.join(", ")
        ));
    }
    let written = write_shims(&hooks, binary)
        .map_err(|e| format!("cannot write shims to {}: {e}", hooks.display()))?;
    println!(
        "{} baked {written} shims into {}",
        valid_sign(),
        hooks.display()
    );
    Ok(())
}

/// Take the shims out of the repository we are standing in.
///
/// Deliberately narrow. It removes files that are OURS and nothing else:
///
/// - a hook we did not write is left alone and named, because somebody wrote it
///   on purpose;
/// - `hook.skip` and `githooks.severity` are never touched — those are the
///   user's statements about their own repository, not our artefacts, and a
///   reinstall should not silently forget that they disabled a check;
/// - the binary goes only when asked, because other repositories are using it.
pub fn uninstall(remove_binary: bool) -> Result<(), String> {
    let Some(hooks) = repo_hooks_dir() else {
        return Err("not inside a git repository".to_string());
    };

    let mut removed = 0usize;
    let mut foreign: Vec<&str> = Vec::new();
    for name in DISPATCHERS {
        let path = hooks.join(name);
        match std::fs::read_to_string(&path) {
            Err(_) => {}
            Ok(text) if is_our_shim(&text) => {
                std::fs::remove_file(&path)
                    .map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
                removed += 1;
            }
            Ok(_) => foreign.push(name),
        }
    }
    println!(
        "{} removed {removed} shims from {}",
        valid_sign(),
        hooks.display()
    );
    if !foreign.is_empty() {
        println!(
            "{} left alone (not ours): {}",
            warning_sign(),
            foreign.join(", ")
        );
    }

    if remove_binary {
        let target = bin_dir().join(installed_name());
        match std::fs::remove_file(&target) {
            Ok(()) => println!(
                "{} removed {}",
                valid_sign(),
                highlight(&target.to_string_lossy())
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot remove {}: {e}", target.display())),
        }
    }

    // Said out loud, because a user who uninstalls and reinstalls should not be
    // surprised that a check they disabled is still disabled.
    println!("    hook.skip and githooks.severity were not touched.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gh-install-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    }

    fn git(dir: &Path, args: &[&str]) {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git");
    }

    /// The embedded shim must be the shim that ships. `include_str!` takes one
    /// of the four; if they ever diverge, the installer would write a file
    /// nobody reviewed.
    #[test]
    fn shims_on_disk_match_the_embedded_one() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates/hooks");
        for name in DISPATCHERS {
            let disk = std::fs::read_to_string(Path::new(dir).join(name))
                .unwrap_or_else(|e| panic!("read {name}: {e}"));
            assert_eq!(disk, SHIM, "{name} differs from the embedded shim");
        }
    }

    /// The whole point of the module. A directory holding tracked files is the
    /// source checkout reached through a symlink, and emptying it destroys work.
    #[test]
    fn a_directory_holding_tracked_files_is_never_safe() {
        let d = tmp("tracked");
        git(&d, &["init", "-q", "--template=", "."]);
        git(&d, &["config", "user.email", "t@t.test"]);
        git(&d, &["config", "user.name", "t"]);
        std::fs::write(d.join("kept.txt"), "precious\n").expect("write");
        git(&d, &["add", "-A"]);
        git(&d, &["commit", "-qm", "seed"]);

        assert_eq!(classify_dir(&d), TemplateDir::IsCheckout);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A path comparison against the source tree passes here and is WRONG: a
    /// worktree is a different path holding the same tracked files. This is the
    /// case that caused the second incident.
    #[test]
    fn a_worktree_is_recognised_even_though_its_path_differs() {
        let d = tmp("wt-main");
        git(&d, &["init", "-q", "--template=", "."]);
        git(&d, &["config", "user.email", "t@t.test"]);
        git(&d, &["config", "user.name", "t"]);
        std::fs::write(d.join("kept.txt"), "precious\n").expect("write");
        git(&d, &["add", "-A"]);
        git(&d, &["commit", "-qm", "seed"]);

        let wt = d.with_extension("wt");
        let _ = std::fs::remove_dir_all(&wt);
        git(&d, &["worktree", "add", "-q", wt.to_str().unwrap()]);
        assert!(
            wt.join("kept.txt").is_file(),
            "worktree did not materialise"
        );
        assert_ne!(d.canonicalize().ok(), wt.canonicalize().ok());
        assert_eq!(
            classify_dir(&wt),
            TemplateDir::IsCheckout,
            "a worktree must be refused exactly like the main checkout"
        );
        let _ = std::fs::remove_dir_all(&wt);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Inside a checkout but tracking nothing here — still not ours to empty.
    #[test]
    fn an_untracked_directory_inside_a_checkout_is_refused() {
        let d = tmp("inside");
        git(&d, &["init", "-q", "--template=", "."]);
        let sub = d.join("scratch");
        std::fs::create_dir_all(&sub).expect("mkdir");
        assert_eq!(classify_dir(&sub), TemplateDir::InsideCheckout);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_ordinary_directory_is_safe() {
        let d = tmp("plain");
        assert_eq!(classify_dir(&d), TemplateDir::Safe);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_directory_is_unresolvable_not_safe() {
        assert_eq!(
            classify_dir(Path::new("/nonexistent-install-c8f2/hooks")),
            TemplateDir::Unresolvable
        );
    }

    /// Baking replaces every occurrence and is idempotent.
    #[test]
    fn baking_is_total_and_idempotent() {
        let once = bake(SHIM, "/opt/githooks");
        assert!(!once.contains(PLACEHOLDER), "a token survived baking");
        assert!(once.contains("/opt/githooks"));
        assert_eq!(bake(&once, "/other"), once, "re-baking must be a no-op");
    }

    /// The shim's comment must not spell the token out, or a global replace
    /// turns the explanation into a machine path — which it did, in every shim
    /// baked before this module existed.
    #[test]
    fn baking_does_not_rewrite_the_comment_explaining_it() {
        for line in bake(SHIM, "/opt/githooks").lines() {
            if line.trim_start().starts_with('#') {
                assert!(
                    !line.contains("/opt/githooks"),
                    "baking rewrote a comment: {line}"
                );
            }
        }
    }

    /// Only an absolute path may be baked.
    ///
    /// A relative one is resolved by the shim against the WORKING TREE, so a
    /// repository shipping an executable by that name would be running it on
    /// the first commit after clone.
    #[test]
    fn only_an_absolute_path_is_bakeable() {
        for good in [
            "/opt/githooks",
            "/home/u/.local/bin/githooks",
            "C:/Users/u/githooks.exe",
            "C:\\Users\\u\\githooks.exe",
        ] {
            assert!(is_bakeable(good), "{good} should be bakeable");
        }
        for bad in [
            "",
            PLACEHOLDER,
            "githooks",
            "./githooks",
            "../githooks",
            "target/debug/githooks",
            "C:githooks.exe",
        ] {
            assert!(!is_bakeable(bad), "{bad:?} must not be bakeable");
        }
    }

    /// The shim must never hand the unsubstituted token to `[ -x ]`: that is a
    /// filesystem question asked in the repository's own directory.
    #[test]
    fn the_shim_never_tests_the_placeholder_as_a_path() {
        assert!(
            !SHIM.contains(&format!("[ -x \"{PLACEHOLDER}\" ]")),
            "the shim tests the raw token as a path"
        );
        assert!(
            SHIM.contains("case \"$BAKED\" in"),
            "the shim lost its absoluteness guard"
        );
    }

    /// Refusing beats writing four hooks that cannot resolve their binary.
    #[test]
    fn write_shims_refuses_a_relative_binary_path() {
        let d = tmp("relative");
        let err = write_shims(&d, "target/debug/githooks").expect_err("must refuse");
        assert!(err.to_string().contains("absolute"), "{err}");
        for name in DISPATCHERS {
            assert!(!d.join(name).exists(), "{name} was written anyway");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Every hook git invokes gets a file, and each is the baked shim.
    #[test]
    fn writing_shims_covers_every_dispatcher() {
        let d = tmp("write");
        let n = write_shims(&d, "/opt/githooks").expect("write");
        assert_eq!(n, DISPATCHERS.len());
        for name in DISPATCHERS {
            let got = std::fs::read_to_string(d.join(name)).expect("read");
            assert!(!got.contains(PLACEHOLDER), "{name} was written unbaked");
            assert!(got.contains("/opt/githooks"), "{name} has no path");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Windows builds githooks.exe, and a shim testing `[ -x .../githooks ]` is
    /// false for it — so the installed name has to keep the suffix. Asserted
    /// against explicit paths, because a `cfg!(windows)` branch is vacuous on
    /// the platform this is usually run on.
    #[test]
    fn the_installed_name_keeps_the_platform_suffix() {
        assert_eq!(
            name_for(Path::new("/w/target/release/githooks.exe")),
            "githooks.exe"
        );
        assert_eq!(
            name_for(Path::new("/u/target/release/githooks")),
            "githooks"
        );
        // A path that happens to contain a dot elsewhere is not an extension.
        assert_eq!(name_for(Path::new("/some.dir/githooks")), "githooks");
    }
}
