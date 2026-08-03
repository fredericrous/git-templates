//! pre-commit-ruff and pre-commit-pyright — the two Python hooks.
//!
//! Both are scoped: they only fire when the repo opts in (a ruff/pyright config
//! or the matching `[tool.*]` table), so a JS repo never pulls a Python tool.
//!
//! Both prefer the repo's PINNED tool over an ambient latest, in the same order
//! the shell versions established: `uv run --no-sync` (the lockfile-pinned one
//! CI runs) → the worktree's .venv → the MAIN worktree's .venv (a linked
//! worktree has none of its own) → PATH → uvx (unpinned LATEST, with a warning,
//! because it flags issues the CI-pinned version does not — phantom failures).

use super::common::{
    fail, fixing_enabled, hl, ok, repo_root, restage, run as run_tool, run_quiet, staged_files,
    warn, which, Restaged,
};
use crate::check::Outcome;
use crate::git;
use std::path::Path;
use std::process::{Command, Stdio};

/// The extensions both Python checks consume. Exported so `registry.rs`
/// declares the scope from the same constant — see `lint_json_yaml::EXTS` for
/// the drift this prevents.
pub const EXTS: &[&str] = &[".py", ".pyi"];

fn tool_runs(root: &str, argv: &[&str]) -> bool {
    let Some((p, rest)) = argv.split_first() else {
        return false;
    };
    Command::new(p)
        .args(rest)
        .arg("--version")
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn main_worktree_venv(tool: &str) -> Option<String> {
    let common = git::stdout(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let p = Path::new(&common).parent()?.join(".venv/bin").join(tool);
    p.is_file().then(|| p.to_string_lossy().into_owned())
}

/// Returns (argv, warned_about_unpinned).
fn resolve_python_tool(root: &str, tool: &str) -> Option<(Vec<String>, bool)> {
    if which("uv").is_some() && tool_runs(root, &["uv", "run", "--no-sync", tool]) {
        return Some((
            vec!["uv".into(), "run".into(), "--no-sync".into(), tool.into()],
            false,
        ));
    }
    let local = format!("{root}/.venv/bin/{tool}");
    if Path::new(&local).is_file() {
        return Some((vec![local], false));
    }
    if let Some(v) = main_worktree_venv(tool) {
        return Some((vec![v], false));
    }
    if which(tool).is_some() {
        return Some((vec![tool.into()], false));
    }
    if which("uvx").is_some() {
        return Some((vec!["uvx".into(), tool.into()], true));
    }
    None
}

fn opts_in(root: &str, configs: &[&str], table: &str) -> bool {
    if configs.iter().any(|c| Path::new(root).join(c).is_file()) {
        return true;
    }
    std::fs::read_to_string(Path::new(root).join("pyproject.toml"))
        .map(|t| t.lines().any(|l| l.trim_start().starts_with(table)))
        .unwrap_or(false)
}

pub fn ruff(_args: &[std::ffi::OsString]) -> Outcome {
    let files = staged_files(EXTS);
    if files.is_empty() {
        return Outcome::Passed;
    }
    let root = repo_root();
    if !opts_in(&root, &["ruff.toml", ".ruff.toml"], "[tool.ruff") {
        return Outcome::Passed;
    }
    let Some((argv, unpinned)) = resolve_python_tool(&root, "ruff") else {
        warn("ruff config found but no ruff/uvx binary. Install ruff or uv.");
        return Outcome::Unavailable;
    };
    if unpinned {
        warn(&format!(
            "No pinned ruff found (.venv); using {} (latest) — may flag issues the CI-pinned ruff doesn't.",
            hl("uvx ruff")
        ));
    }

    // The registry has always declared `Fix::Rewrite` for this check, and
    // `githooks list --json` reported `"fix":"rewrite"` — which `agents_md`
    // explicitly tells agents to trust — while no fixing code existed
    // anywhere. Only prettier and the manifest's externals ever called
    // `restage`. Rather than downgrade the declaration, the fixing is now
    // real.
    //
    // Repair FIRST and QUIETLY, then let the check passes below decide. Ruff
    // legitimately leaves findings it cannot fix, unlike `cargo fmt`, so the
    // repair pass's own exit code says nothing about the verdict — and running
    // it loudly would print the surviving offenders twice.
    let mut repaired = false;
    if fixing_enabled() {
        let _ = run_quiet(&root, &argv, &with_files(&["check", "--fix"], &files));
        let _ = run_quiet(&root, &argv, &with_files(&["format"], &files));
        match restage(&files) {
            Restaged::Staged => repaired = true,
            Restaged::Failed(stuck) => {
                fail(&format!(
                    "ruff rewrote these files but {} failed — the index still holds the OLD \
                     content: {}",
                    hl("git add"),
                    stuck.join(", ")
                ));
                return Outcome::Failed;
            }
            Restaged::Nothing => {}
        }
    }

    let mut failed = false;
    if !run_tool(&root, &argv, &with_files(&["check"], &files)) {
        fail(&format!(
            "Ruff lint issues. Run {}. Offenders above.",
            hl("ruff check --fix")
        ));
        failed = true;
    }
    if !run_tool(&root, &argv, &with_files(&["format", "--check"], &files)) {
        fail(&format!(
            "Ruff found unformatted files. Run {} on the files listed above.",
            hl("ruff format")
        ));
        failed = true;
    }

    if failed {
        // A repair that could not finish the job still blocks — and whatever it
        // DID fix is already staged, so the next attempt starts from there.
        return Outcome::Failed;
    }
    if repaired {
        ok("Ruff fixed and re-staged");
        return Outcome::Fixed;
    }
    // An unpinned ruff RAN, and its verdict was clean — that is a pass, not a
    // gap. The caveat above is advice about which ruff spoke, not a claim that
    // none did.
    ok("Ruff passed");
    Outcome::Passed
}

/// `<sub…> --force-exclude -- <files>`.
///
/// `--force-exclude` on every pass so ruff honours the project's `exclude`
/// even though the paths are handed to it explicitly. `--` before the file
/// list because a staged file named e.g. `-x.py` would otherwise be read as a
/// flag by ruff's own parser.
fn with_files(sub: &[&str], files: &[String]) -> Vec<String> {
    let mut argv: Vec<String> = sub.iter().map(|s| (*s).to_string()).collect();
    argv.push("--force-exclude".into());
    argv.push("--".into());
    argv.extend(files.iter().cloned());
    argv
}

pub fn pyright(_args: &[std::ffi::OsString]) -> Outcome {
    let files = staged_files(EXTS);
    if files.is_empty() {
        return Outcome::Passed;
    }
    let root = repo_root();
    if !opts_in(
        &root,
        &["pyrightconfig.json", "pyrightconfig.jsonc"],
        "[tool.pyright",
    ) {
        return Outcome::Passed;
    }
    let Some((argv, _)) = resolve_python_tool(&root, "pyright") else {
        warn("pyright config found but no pyright binary. Install pyright or uv.");
        return Outcome::Unavailable;
    };
    // Scoped to the STAGED files, not the whole tree: fast enough for a
    // pre-commit hook while catching the per-file errors that are the usual
    // local-vs-CI gap. Pyright still resolves the whole workspace for imports,
    // so inference is unchanged; only the reported set is scoped. CI's
    // whole-tree run stays the authority for cross-file-only errors.
    // `--` before the file list: a staged file named e.g. `-p.py` would
    // otherwise be read as a flag by pyright's own parser.
    let mut with_files = vec!["--".to_string()];
    with_files.extend(files.iter().cloned());
    if !run_tool(&root, &argv, &with_files) {
        fail("Pyright type errors. Please fix");
        return Outcome::Failed;
    }
    ok("pyright passed");
    Outcome::Passed
}
