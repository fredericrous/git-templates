//! Walking a directory tree for git repositories, and counting everything.
//!
//! The counters are the point. `scripts/propagate.sh` printed scalars with no
//! denominator, and twice reported something that could not be true: 192
//! removals per hook across 96 repos holding one copy each, and
//! `0 copies / 0 distinct` from a `-maxdepth` that matched nothing. The second
//! is the dangerous one — a broken scan and a clean fleet produced identical
//! output.
//!
//! So this records how it arrived at its answer: directories visited, entries
//! it could not read, subtrees it deliberately skipped. A caller can then tell
//! "found nothing because there is nothing" from "found nothing because I
//! looked in the wrong place", which no scalar can express.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::shim::{self, BakeState, ShimState, DISPATCHERS};
use crate::skips::{self, SkipEntry};

/// Subtrees never worth descending. Matches the exclusions the shell sweep
/// used, so the two agree about what "the fleet" means.
pub const EXCLUDED: [&str; 6] = ["node_modules", "target", "dist", "build", ".venv", "vendor"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Repo {
    /// Path relative to the scan root, which is what a human recognises.
    pub path: PathBuf,
    /// At least one file in `.git/hooks` dispatches to the binary.
    pub managed: bool,
    /// One entry per git-invoked hook, in `DISPATCHERS` order.
    pub shims: Vec<ShimState>,
    pub baked: BakeState,
    /// Our files that we no longer ship — the 16 per-check shims retired when
    /// checks moved in-process, and anything else removed upstream.
    pub stale_ours: Vec<String>,
    /// Hand-written `pre-commit-*` / `pre-push-*` sub-hooks. Nothing dispatches
    /// these any more, so they LOOK installed and never run.
    pub foreign_subs: Vec<String>,
    /// The node-era `package.json` that forced CommonJS. No hook is node now.
    pub hook_pkgjson: bool,
    /// Manifests present at the repo root. Display only — the LANG column.
    pub languages: Vec<String>,
    /// Checks that would ever fire here, from each check's own `Scope`
    /// evaluated against this repo's tracked files. Not inferred by this crate:
    /// a fourth copy of that rule was what `LANGUAGES` used to be.
    pub applicable: Vec<String>,
    /// `hook.skip` entries, resolved: what each one suppresses and where it
    /// came from. Bare strings hid both — a value is a SUBSTRING pattern, not a
    /// check name, and local/global are indistinguishable once merged.
    pub skips: Vec<SkipEntry>,
}

/// A whole scan, including how it was performed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FleetScan {
    pub root: PathBuf,
    pub depth: usize,
    pub git_dirs_found: usize,
    pub hook_dirs_seen: usize,
    pub managed_seen: usize,
    pub unmanaged_seen: usize,
    /// Paths that exist but could not be read. Never silently dropped: an
    /// unreadable repo is not an absent one.
    pub unreadable: Vec<PathBuf>,
    pub excluded_dirs: usize,
    pub dirs_visited: usize,
    pub repos: Vec<Repo>,
}

impl FleetScan {
    /// True when the scan looked at essentially nothing. The caller is expected
    /// to render this as a FAILURE rather than as an empty success — the single
    /// rule this whole tool exists to enforce.
    pub fn looks_like_a_failed_scan(&self) -> bool {
        self.git_dirs_found == 0
    }
}

/// A file is ours only if it dispatches to the binary. Anything else in
/// `.git/hooks` is somebody's own hook and is never counted as ours.
///
/// This was deliberately identical to the test `propagate.sh` used, so the two
/// could not drift on what "managed" means while both existed. The script is
/// gone; the definition stays because it is the right one, not because
/// something else depends on it.
fn is_ours(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .map(|s| s.contains("--hooks-dir"))
        .unwrap_or(false)
}

fn is_managed(hooks: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(hooks) else {
        return false;
    };
    entries.flatten().any(|e| {
        let p = e.path();
        p.is_file() && is_ours(&p)
    })
}

/// Progress emitted while walking, so a caller can paint rows as they are
/// found rather than after 7 seconds of nothing. Visibility of system status is
/// the first usability heuristic, and this scan is well past the ~400ms at
/// which an interface stops feeling immediate.
pub enum Progress<'a> {
    Visited(usize),
    Found(&'a Repo),
}

pub fn scan(root: &Path, depth: usize, installed_binary: &str) -> FleetScan {
    scan_with(root, depth, installed_binary, &mut |_| {})
}

pub fn scan_with(
    root: &Path,
    depth: usize,
    installed_binary: &str,
    on: &mut dyn FnMut(Progress),
) -> FleetScan {
    let mut s = FleetScan {
        root: root.to_path_buf(),
        depth,
        git_dirs_found: 0,
        hook_dirs_seen: 0,
        managed_seen: 0,
        unmanaged_seen: 0,
        unreadable: Vec::new(),
        excluded_dirs: 0,
        dirs_visited: 0,
        repos: Vec::new(),
    };
    walk(root, root, depth, installed_binary, on, &mut s);
    s.repos.sort_by(|a, b| a.path.cmp(&b.path));
    s
}

fn walk(
    root: &Path,
    dir: &Path,
    budget: usize,
    installed_binary: &str,
    on: &mut dyn FnMut(Progress),
    s: &mut FleetScan,
) {
    s.dirs_visited += 1;
    on(Progress::Visited(s.dirs_visited));
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => {
            s.unreadable.push(dir.to_path_buf());
            return;
        }
    };

    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();

        if name == ".git" {
            s.git_dirs_found += 1;
            let hooks = path.join("hooks");
            if hooks.is_dir() {
                s.hook_dirs_seen += 1;
            }
            let managed = is_managed(&hooks);
            if managed {
                s.managed_seen += 1;
            } else {
                s.unmanaged_seen += 1;
            }
            let repo = path.parent().unwrap_or(&path);
            let found = inspect(root, repo, &hooks, managed, installed_binary);
            on(Progress::Found(&found));
            s.repos.push(found);
            // A repository is a leaf for this purpose; nothing inside .git is
            // another repo, and worktrees keep their hooks in the main one.
            continue;
        }

        if EXCLUDED.contains(&name.as_str()) {
            s.excluded_dirs += 1;
            continue;
        }
        subdirs.push(path);
    }

    if budget == 0 {
        return;
    }
    for d in subdirs {
        walk(root, &d, budget - 1, installed_binary, on, s);
    }
}

/// Everything we can learn about one repository from its hooks directory.
fn inspect(root: &Path, repo: &Path, hooks: &Path, managed: bool, installed_binary: &str) -> Repo {
    let shims: Vec<ShimState> = DISPATCHERS
        .iter()
        .map(|n| shim::classify(&hooks.join(n)))
        .collect();

    let (mut stale_ours, mut foreign_subs) = (Vec::new(), Vec::new());
    let mut hook_pkgjson = false;
    if let Ok(entries) = std::fs::read_dir(hooks) {
        for e in entries.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".sample") || DISPATCHERS.contains(&name.as_str()) {
                continue;
            }
            if name == "package.json" {
                hook_pkgjson = std::fs::read_to_string(&p)
                    .map(|c| c.contains("Forces Node"))
                    .unwrap_or(false);
                continue;
            }
            if is_ours(&p) {
                stale_ours.push(name);
            } else if name.starts_with("pre-commit-") || name.starts_with("pre-push-") {
                foreign_subs.push(name);
            }
        }
    }
    stale_ours.sort();
    foreign_subs.sort();

    Repo {
        path: repo.strip_prefix(root).unwrap_or(repo).to_path_buf(),
        managed,
        baked: shim::bake_state(&shims, installed_binary),
        shims,
        stale_ours,
        foreign_subs,
        hook_pkgjson,
        languages: languages(repo),
        applicable: applicable_checks(repo),
        skips: skips::read(repo),
    }
}

/// Root-level manifests only. Deliberately an approximation: the hooks
/// themselves resolve the NEAREST manifest, so a repo can hold Rust in a
/// subdirectory and show no `rust` here. It drives a display column, never a
/// verdict.
fn languages(repo: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let has = |f: &str| repo.join(f).is_file();
    if has("Cargo.toml") {
        out.push("rust".into());
    }
    if has("package.json") {
        out.push("js".into());
    }
    if has("pyproject.toml") || has("requirements.txt") || has("setup.py") {
        out.push("python".into());
    }
    if has("kustomization.yaml") || has("kustomization.yml") || repo.join("k8s").is_dir() {
        out.push("k8s".into());
    }
    out
}

/// Which checks could ever fire in this repository.
///
/// One `git ls-files` per repo, evaluated against each check's declared
/// `Scope`. Coarser than what a check enforces at commit time — `cargo-fmt`
/// resolves the NEAREST ancestor `Cargo.toml` while this asks whether the repo
/// contains one at all — and deliberately so: the dispatcher answers "does this
/// apply to these staged files", and this answers "would it ever fire here".
/// Over-approximating is the safe direction; the alternative was a table in
/// this crate guessing at rules the checks already own.
fn applicable_checks(repo: &Path) -> Vec<String> {
    let Ok(out) = Command::new("git")
        .args(["ls-files"])
        .current_dir(repo)
        .output()
    else {
        return Vec::new();
    };
    let paths: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .collect();
    githooks_runtime::registry::CHECKS
        .iter()
        .filter(|c| c.scope.matches(&paths))
        .map(|c| c.name.to_string())
        .collect()
}
