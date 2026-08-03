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

use crate::hookfile::{self, HookFile, Refuse, Staged, SwapFailure};
use crate::ui::{error_sign, highlight, valid_sign, warning_sign};

/// The token every shim carries until it is baked.
pub const PLACEHOLDER: &str = "__GITHOOKS_BIN__";

/// The one shim. All four git-invoked hooks are the same file — it passes its
/// own filename through — and `shims_on_disk_match_the_embedded_one` keeps the
/// repository's `templates/hooks/` honest against this copy.
///
/// The canonical text lives INSIDE this crate, and that is a packaging
/// constraint rather than a preference. It used to be
/// `include_str!("../../../templates/hooks/pre-commit")`, reaching up to the
/// repository root — which works in a checkout and cannot work in a published
/// crate, because `cargo package` tars up this directory and nothing above it.
/// The tarball compiled nowhere: `couldn't read src/../../../templates/hooks/
/// pre-commit`. crates.io is immutable, so that would have been a broken
/// release that could only be yanked, never fixed in place.
///
/// The repository's `templates/hooks/` still holds the four installable copies
/// — that directory IS the product for anyone pointing `init.templateDir` at a
/// clone, and it has to be real files rather than symlinks because Git for
/// Windows materialises those as text files containing a path.
pub const SHIM: &str = include_str!("../templates/hooks/pre-commit");

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

/// The one directory an UNBAKED shim looks in, hardcoded in the shim itself.
///
/// Deliberately NOT `bin_dir()`, and the difference is the whole point of
/// `warn_if_unbaked_cannot_resolve`. `bin_dir()` answers "where should install
/// PUT the binary?" and honours `$GITHOOKS_BIN_DIR`. This answers "where will a
/// shim that never got a path baked into it LOOK?", which is a constant in a
/// POSIX sh file and cannot be configured at all.
///
/// `$GITHOOKS_BIN_DIR` is deliberately not wired into the shim to close that
/// gap. It is an install-time question answered in the shell where `githooks
/// install` ran; the shim runs inside git's environment during a commit, where
/// that variable is almost never set — so honouring it there would ship a knob
/// that looks like it works and silently does not. The runtime override already
/// exists and is `$GIT_HOOKS_BIN`; a second variable able to redirect which
/// binary executes on every commit would double that surface for nothing.
///
/// Not "XDG", either: the XDG Base Directory spec defines no binary directory.
/// `~/.local/bin` is simply the widely-observed convention.
fn unbaked_lookup_dir() -> PathBuf {
    home().join(".local").join("bin")
}

/// Say so when the binary went somewhere an unbaked shim will never look.
///
/// Two supported choices stop composing when made together. A template dir that
/// IS the checkout keeps the placeholder on purpose — those shims resolve the
/// binary at run time from [`unbaked_lookup_dir`]. Install to a custom
/// `$GITHOOKS_BIN_DIR` as well and nothing baked a path, while the one path the
/// shim knows is not where the binary went.
///
/// Nothing is silently skipped — the shim prints what it looked at and exits 1,
/// which fails the commit loudly. But it fails at somebody's next commit, in a
/// repository they have not thought about since, and the cause is a decision
/// made here. So it is said here.
fn warn_if_unbaked_cannot_resolve(binary: &str) {
    let looked = unbaked_lookup_dir();
    let placed = Path::new(binary).parent();

    // Compare RESOLVED paths, and require both to resolve. `a.ok() == b.ok()`
    // would read `None == None` as "the same directory" — the shape of a bug
    // this module has already had once, in `already_there`.
    let reachable = placed.is_some_and(|p| {
        matches!(
            (p.canonicalize(), looked.canonicalize()),
            (Ok(a), Ok(b)) if a == b
        )
    });
    if reachable {
        return;
    }

    println!();
    println!(
        "{} the binary is at {}, which an unbaked shim will not find.",
        warning_sign(),
        highlight(binary)
    );
    println!(
        "    Shims here keep the placeholder, and they look only in {}.",
        looked.display()
    );
    println!("    Either link it where they look:");
    println!(
        "      ln -s {} {}",
        binary,
        looked.join("githooks").display()
    );
    println!("    or set GIT_HOOKS_BIN in the environment git runs hooks with:");
    println!("      export GIT_HOOKS_BIN={binary}");
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

/// Hook files in `dir` that exist and are NOT ours, each with its reason.
///
/// `install` used to write all four unconditionally, which silently destroyed a
/// `commit-msg` somebody had written themselves. That is the same failure as the
/// two that overwrote tracked files, one directory along, and it had no guard at
/// all — the fleet's `fix` planner has one and the per-repo installer never did.
///
/// It carries the [`HookFile`] rather than only the name because "commit-msg is
/// not ours" sends somebody to diff a file against a shim they have never seen,
/// while "commit-msg is not valid UTF-8 — a compiled hook, probably" ends the
/// question. The old version could not have said either: it read the file as a
/// string, and a file it could not read came back as NOT foreign.
fn foreign_hooks(dir: &Path) -> Vec<(&'static str, HookFile)> {
    DISPATCHERS
        .into_iter()
        .map(|name| (name, hookfile::classify(&dir.join(name))))
        .filter(|(_, what)| !matches!(what, HookFile::Absent | HookFile::Ours))
        .collect()
}

/// One dispatcher that landed, and what stood at that path before it.
///
/// `replaced` exists so `--force` can say what it took. It printed
/// "baked 4 shims" and nothing else, which is a receipt for an act whose whole
/// point is that it destroys something — the user typed `--force` precisely
/// because there was a file there, and the one thing the output never said was
/// which files or what they were.
#[derive(Debug)]
pub struct Written {
    pub path: PathBuf,
    pub replaced: HookFile,
}

/// Why a set of shims was not written.
#[derive(Debug)]
pub enum ShimWriteError {
    /// One or more paths are not ours to write. Every refusal is carried, not
    /// just the first: somebody about to run `--force` should see all four.
    Refused(Vec<Refuse>),
    /// A failure before any destination was touched — an unbakeable path, or a
    /// staging write that could not happen (no space, no permission).
    Preflight { at: PathBuf, error: std::io::Error },
    /// Staging succeeded and a rename did not. The only failure mode that can
    /// leave a directory partly written, which is why it carries the lists.
    Swap(SwapFailure),
}

impl std::fmt::Display for ShimWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShimWriteError::Refused(refusals) => {
                writeln!(f, "refusing to write {} hooks:", refusals.len())?;
                for (i, r) in refusals.iter().enumerate() {
                    if i > 0 {
                        writeln!(f)?;
                    }
                    write!(f, "    {}", r.explain())?;
                }
                Ok(())
            }
            ShimWriteError::Preflight { at, error } => {
                write!(f, "cannot prepare {}: {error}", at.display())
            }
            ShimWriteError::Swap(s) => write!(f, "{s}"),
        }
    }
}

/// Write the four dispatchers into `dir`: guard ALL, stage ALL, then swap.
///
/// Three phases, in that order, because the posture `bake_repo_hooks` has
/// claimed since it was written — "fail closed, and for the whole repository
/// rather than per file: a partial install is how a repo ends up with two of
/// four hooks and no way to tell" — was a comment over a loop that checked one
/// file and then wrote it, four times. A refusal on the third hook came after
/// two had already been overwritten.
///
/// Now: every path is guarded before any body is written, and every body is
/// written before any destination is touched. A refusal anywhere means nothing
/// at all was written; a staging failure likewise. Only the swap can leave a
/// directory partly done, and that is reported as exactly which files landed
/// and which did not (see [`SwapFailure`]) rather than as a count.
///
/// Returns what was written and what each write replaced, so `--force` can name
/// what it took.
fn write_shims(dir: &Path, bin: &str, force: bool) -> Result<Vec<Written>, ShimWriteError> {
    // Fail closed rather than write four hooks that resolve to nothing — or, if
    // the repository happens to hold a file by that name, to something.
    if !is_bakeable(bin) {
        return Err(ShimWriteError::Preflight {
            at: dir.to_path_buf(),
            error: std::io::Error::other(format!(
                "refusing to bake {bin:?}: the shim takes an absolute path only"
            )),
        });
    }
    let baked = bake(SHIM, bin);

    // Phase 1 — guard every path. Nothing has been written and nothing will be
    // if a single one of these refuses.
    let mut allowed: Vec<(PathBuf, HookFile)> = Vec::new();
    let mut refusals: Vec<Refuse> = Vec::new();
    for name in DISPATCHERS {
        let path = dir.join(name);
        match hookfile::guard_write(&path, force) {
            Ok(what) => allowed.push((path, what)),
            Err(r) => refusals.push(r),
        }
    }
    if !refusals.is_empty() {
        return Err(ShimWriteError::Refused(refusals));
    }

    // Phase 2 — stage every body. A `Staged` that never lands removes its own
    // temporary on drop, so an error here leaves the directory as it was.
    let mut staged: Vec<Staged> = Vec::new();
    for (path, _) in &allowed {
        match hookfile::stage(path, &baked, true) {
            Ok(s) => staged.push(s),
            Err(error) => {
                return Err(ShimWriteError::Preflight {
                    at: path.clone(),
                    error,
                });
            }
        }
    }

    // Phase 3 — swap. Renames, so a symlinked destination is REPLACED rather
    // than written through.
    hookfile::commit_all(staged).map_err(ShimWriteError::Swap)?;
    Ok(allowed
        .into_iter()
        .map(|(path, replaced)| Written { path, replaced })
        .collect())
}

/// For the installed BINARY only — shims get their mode from `hookfile::stage`,
/// before they are anywhere a hook could be dispatched from.
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
    populate_template_dir(&binary, force)?;
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
    // No repository, no manifest to ask about. `repo_root()` answered "." and
    // this went looking for `./.githooks.conf` in whatever directory the
    // install was run from — a file it would then have offered to trust ON
    // BEHALF of a repository that does not exist.
    let Ok(root) = crate::hooks::common::repo_root_checked() else {
        return;
    };
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
    // Same reason as `offer_trust`: with `repo_root()`'s "." fallback, an
    // install run outside a repository offered to write an AGENTS.md into the
    // current directory — the one thing `install` writes to TRACKED content,
    // aimed at a directory nobody said was a project.
    let Ok(root) = crate::hooks::common::repo_root_checked() else {
        return;
    };
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
    //
    // `me.canonicalize().ok() == target.canonicalize().ok()` is the version
    // this replaces, and it was wrong in the one case that matters: when the
    // TARGET does not exist yet — a first install, the whole point of the
    // step — `canonicalize` returns `Err`, both sides are `None`, `None ==
    // None` is true, and the copy was skipped. The binary was never installed,
    // and `install` printed "installed <path>" for a file that was not there.
    // Every shim then baked that path and resolved nothing. Two `Ok`s that
    // agree is the only thing that means "same file".
    let already_there = matches!(
        (me.canonicalize(), target.canonicalize()),
        (Ok(a), Ok(b)) if a == b
    );
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
fn populate_template_dir(binary: &str, force: bool) -> Result<(), String> {
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
            // …as long as run-time resolution can actually reach the binary,
            // which the sentence above used to assert unconditionally.
            warn_if_unbaked_cannot_resolve(binary);
        }
        TemplateDir::InsideCheckout => {
            println!(
                "{} {shown} is inside a git checkout — leaving it alone.",
                warning_sign()
            );
            warn_if_unbaked_cannot_resolve(binary);
        }
        TemplateDir::NoGit => println!(
            "{} git is not on PATH — refusing to delete anything.",
            warning_sign()
        ),
        TemplateDir::Unresolvable => {
            println!("{} cannot resolve {shown} — skipping.", warning_sign())
        }
        TemplateDir::Safe => {
            let written = write_shims(&dir, binary, force)
                .map_err(|e| format!("cannot write shims to {shown}: {e}"))?;
            println!("{} wrote {} shims to {shown}", valid_sign(), written.len());
            report_overwrites(&written);
        }
    }
    Ok(())
}

/// Say what each write took, for the writes that took something.
///
/// `install --force` used to print `baked 4 shims` and stop. `--force` is
/// typed precisely because a file is in the way, so the one fact the output
/// omitted is the only fact the user needed: which files, and what they were.
/// A hook replaced with no record of what it was is unrecoverable — `.git` is
/// not tracked, so there is nothing to `git checkout` it back from.
fn report_overwrites(written: &[Written]) {
    for w in written {
        if matches!(w.replaced, HookFile::Absent | HookFile::Ours) {
            continue;
        }
        println!(
            "{} overwrote {} — it was {}",
            warning_sign(),
            w.path.display(),
            w.replaced.describe()
        );
    }
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
///
/// The tracked guard is inherited from `hookfile::guard_write` rather than
/// written here, and that inheritance closes a verified bug: with
/// `.git/hooks/pre-commit` a symlink to a TRACKED `devhooks/pre-commit`,
/// `install --force` rewrote the tracked source file. Every guard this function
/// had was about the LINK path — untracked, inside `.git`, unremarkable — while
/// `fs::write` followed the link and landed in the working tree. Both halves
/// are fixed at once: the symlink is refused by name, and `--force` replaces
/// the link by rename instead of writing through it.
fn bake_repo_hooks(binary: &str, force: bool) -> Result<(), String> {
    let Some(hooks) = repo_hooks_dir() else {
        println!(
            "{} not inside a git repository — no repo hooks written.",
            warning_sign()
        );
        return Ok(());
    };
    let _ = std::fs::create_dir_all(&hooks);

    // Asked here, ahead of the guard, only so the message can offer `--force`.
    // The guard inside `write_shims` is the one that decides, and it refuses
    // things `--force` will not move (a tracked path, a path git cannot answer
    // for) which this pre-check deliberately says nothing about.
    let foreign = foreign_hooks(&hooks);
    if !foreign.is_empty() && !force {
        let mut msg = format!(
            "{} {} already has hooks that are not ours:",
            error_sign(),
            hooks.display()
        );
        for (name, what) in &foreign {
            msg.push_str(&format!("\n    {name} — {}", what.describe()));
        }
        msg.push_str("\n    Look at them first, then `githooks install --force`.");
        return Err(msg);
    }

    let written = write_shims(&hooks, binary, force)
        .map_err(|e| format!("cannot write shims to {}: {e}", hooks.display()))?;
    println!(
        "{} baked {} shims into {}",
        valid_sign(),
        written.len(),
        hooks.display()
    );
    report_overwrites(&written);
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
    // The template directory FIRST, and unconditionally, because it is the only
    // part of an install that keeps working when you are not standing in a
    // repository — and because `uninstall` returning early with "not inside a
    // git repository" is how the standing grant survived every attempt to
    // revoke it.
    uninstall_template_dir()?;
    uninstall_repo_hooks()?;

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

    report_global_template_dir();

    // Said out loud, because a user who uninstalls and reinstalls should not be
    // surprised that a check they disabled is still disabled.
    println!("    hook.skip and githooks.severity were not touched.");
    Ok(())
}

/// Remove our shims from the repository we are standing in, naming everything
/// we did not take and why.
///
/// Every non-removal is now NAMED. The loop this replaces matched
/// `Err(_) => {}` on `read_to_string`, so a hook that could not be read at all
/// — a compiled one, or one whose permissions we lack — was passed over in
/// total silence: not removed, not counted, not mentioned. The README's promise
/// that a foreign hook is "left alone and named" was true only for hooks that
/// happened to be valid UTF-8.
///
/// Not being in a repository is a warning rather than an error. It used to
/// return `Err`, which was defensible on its own but became wrong once
/// `uninstall_template_dir` existed: the early return meant that running
/// `githooks uninstall` from a plain directory did nothing AND said nothing,
/// while `init.templateDir` quietly went on installing hooks into every future
/// clone. Refusing is not failing — the same rule `populate_template_dir`
/// already states.
fn uninstall_repo_hooks() -> Result<(), String> {
    let Some(hooks) = repo_hooks_dir() else {
        println!(
            "{} not inside a git repository — no repo hooks removed.",
            warning_sign()
        );
        return Ok(());
    };

    let mut removed = 0usize;
    let mut left: Vec<String> = Vec::new();
    for name in DISPATCHERS {
        let path = hooks.join(name);
        match hookfile::classify(&path) {
            HookFile::Absent => {}
            HookFile::Ours => match hookfile::guard_remove(&path, true) {
                Ok(()) => {
                    hookfile::remove_regular(&path)
                        .map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
                    removed += 1;
                }
                // Ours by marker, and still not ours to delete: a tracked path,
                // or one git could not answer for.
                Err(r) => left.push(r.explain()),
            },
            what => left.push(format!("{name} — {}", what.describe())),
        }
    }
    println!(
        "{} removed {removed} shims from {}",
        valid_sign(),
        hooks.display()
    );
    for reason in &left {
        println!("{} left alone: {reason}", warning_sign());
    }
    Ok(())
}

/// Take our shims back out of the template directory.
///
/// `install` writes there; `uninstall` did not, which meant uninstall did not
/// undo install. Combined with `init.templateDir`, that is the failure worth
/// spelling out: the user runs `githooks uninstall`, sees "removed 4 shims",
/// believes they are done — and every `git clone` and `git init` from then on
/// copies the template directory into the new repository's `.git/hooks` and
/// installs the hooks again. They uninstalled a repository, not a machine.
///
/// The same classification `install` uses decides what may happen here, for the
/// same reason and with the sharper stake: `~/.config/git/git-templates` is
/// commonly a SYMLINK to a checkout of this repository, and "uninstalling"
/// there means `rm` on tracked source. That is not a hypothetical; it is the
/// two incidents this module exists because of, and a delete has no `--force`.
fn uninstall_template_dir() -> Result<(), String> {
    let dir = template_hooks_dir();
    // The RESOLVED path, because the configured one is usually the symlink that
    // hides exactly what we are about to explain.
    let shown = dir.canonicalize().unwrap_or_else(|_| dir.clone());
    let shown = shown.display();

    match classify_dir(&dir) {
        TemplateDir::IsCheckout | TemplateDir::InsideCheckout => {
            println!(
                "{} template dir is a git checkout ({shown}) — deleting NOTHING there.",
                warning_sign()
            );
            println!("    Those shims are tracked files belonging to that checkout,");
            println!("    not something this install put there. Remove them with git,");
            println!("    or point init.templateDir somewhere else.");
        }
        TemplateDir::NoGit => println!(
            "{} git is not on PATH — cannot tell whether {shown} is a checkout, deleting nothing.",
            warning_sign()
        ),
        TemplateDir::Unresolvable => println!(
            "{} no template dir at {shown} — nothing to remove.",
            warning_sign()
        ),
        TemplateDir::Safe => {
            let mut removed = 0usize;
            let mut left: Vec<String> = Vec::new();
            for name in DISPATCHERS {
                let path = dir.join(name);
                match hookfile::classify(&path) {
                    HookFile::Absent => {}
                    HookFile::Ours => match hookfile::guard_remove(&path, true) {
                        Ok(()) => {
                            hookfile::remove_regular(&path)
                                .map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
                            removed += 1;
                        }
                        Err(r) => left.push(r.explain()),
                    },
                    what => left.push(format!("{name} — {}", what.describe())),
                }
            }
            println!("{} removed {removed} shims from {shown}", valid_sign());
            for reason in &left {
                println!("{} left alone: {reason}", warning_sign());
            }
        }
    }
    Ok(())
}

/// Say, every time, whether `init.templateDir` is still pointing at us.
///
/// UNCONDITIONAL, including when the template dir was a checkout we refused to
/// touch and when there was no template dir at all — because the config setting
/// is what actually installs hooks into new repositories, and it survives every
/// file this command removes. An uninstall that leaves it set has not
/// uninstalled anything durable: the next `git clone` re-installs.
///
/// Printed rather than unset. `git config --global` is the user's file, holding
/// their identity and their aliases, and reaching into it uninvited is a larger
/// claim than removing files this tool wrote. The command to run is given
/// verbatim so it is a copy rather than a lookup.
fn report_global_template_dir() {
    let Some(configured) = crate::git::stdout(&["config", "--global", "--get", "init.templateDir"])
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    println!();
    println!(
        "{} init.templateDir is still set: {}",
        warning_sign(),
        highlight(&configured)
    );
    println!("    Every `git clone` and `git init` still copies hooks from there");
    println!("    into the new repository. Uninstalling this repo did not change that.");
    println!("    Undo it with:");
    println!(
        "        {}",
        highlight("git config --global --unset init.templateDir")
    );
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
        // Absent when this crate is built from its PUBLISHED tarball, which
        // contains this directory and nothing above it. There is no drift to
        // catch in that situation — the only shim present is the one compiled
        // in — so say so rather than fail a test about a file that is not
        // supposed to be there.
        if !Path::new(dir).is_dir() {
            println!(
                "! no repository checkout here — nothing to compare the embedded shim against"
            );
            return;
        }
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
        let err = write_shims(&d, "target/debug/githooks", false).expect_err("must refuse");
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
        let written = write_shims(&d, "/opt/githooks", false).expect("write");
        assert_eq!(written.len(), DISPATCHERS.len());
        for name in DISPATCHERS {
            let got = std::fs::read_to_string(d.join(name)).expect("read");
            assert!(!got.contains(PLACEHOLDER), "{name} was written unbaked");
            assert!(got.contains("/opt/githooks"), "{name} has no path");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The whole-repository posture, at the level of the function that owes it:
    /// ONE unwritable path and nothing at all is written. `bake_repo_hooks` has
    /// claimed this in a comment since it was written, over a loop that checked
    /// one file then wrote it, four times over — so a refusal on the third hook
    /// arrived after two were already gone.
    #[test]
    fn one_refusal_writes_nothing_at_all() {
        let d = tmp("all-or-nothing");
        // `prepare-commit-msg` sorts last among the dispatchers, so under the
        // old check-then-write loop the first three would already be on disk by
        // the time this one refused.
        let theirs = d.join("prepare-commit-msg");
        std::fs::write(&theirs, "#!/bin/sh\necho MINE\n").expect("write");

        let err = write_shims(&d, "/opt/githooks", false).expect_err("must refuse");
        assert!(
            matches!(err, ShimWriteError::Refused(ref rs) if rs.len() == 1),
            "{err}"
        );
        for name in ["commit-msg", "pre-commit", "pre-push"] {
            assert!(
                !d.join(name).exists(),
                "{name} was written despite a refusal elsewhere"
            );
        }
        assert_eq!(
            std::fs::read_to_string(&theirs).expect("read"),
            "#!/bin/sh\necho MINE\n"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A refusal has to say WHAT was in the way, not only that something was.
    /// The old predicate could not: it read the file as a string, so the one
    /// case worth naming — a compiled hook — came back as "not foreign" and was
    /// overwritten in silence.
    #[test]
    fn a_refusal_names_the_reason_for_each_hook() {
        let d = tmp("named");
        std::fs::write(d.join("commit-msg"), [0x7f, b'E', b'L', b'F', 0xff]).expect("write");
        std::fs::write(d.join("pre-commit"), "#!/bin/sh\necho mine\n").expect("write");

        let err = write_shims(&d, "/opt/githooks", false).expect_err("must refuse");
        let text = err.to_string();
        assert!(text.contains("not valid UTF-8"), "{text}");
        assert!(text.contains("commit-msg"), "{text}");
        assert!(text.contains("pre-commit"), "{text}");

        // And `foreign_hooks` — which is what phrases the `--force` offer —
        // agrees about both.
        let foreign = foreign_hooks(&d);
        assert_eq!(foreign.len(), 2, "{foreign:?}");
        assert!(foreign
            .iter()
            .any(|(n, w)| *n == "commit-msg" && matches!(w, HookFile::Foreign(_))));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `--force` says what it took, per file, with what it was. Without this
    /// the output was "baked 4 shims" — a receipt with the transaction left
    /// off, for the one operation whose purpose is to destroy something.
    #[test]
    fn force_reports_what_each_write_replaced() {
        let d = tmp("force-report");
        std::fs::write(d.join("commit-msg"), "#!/bin/sh\necho mine\n").expect("write");
        let written = write_shims(&d, "/opt/githooks", true).expect("force must write");
        let replaced: Vec<_> = written
            .iter()
            .filter(|w| !matches!(w.replaced, HookFile::Absent))
            .collect();
        assert_eq!(replaced.len(), 1, "{written:?}");
        assert!(replaced[0].path.ends_with("commit-msg"));
        assert!(matches!(replaced[0].replaced, HookFile::Foreign(_)));
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
