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

use serde::Serialize;

/// Subtrees never worth descending. Matches the exclusions the shell sweep
/// used, so the two agree about what "the fleet" means.
pub const EXCLUDED: [&str; 6] = ["node_modules", "target", "dist", "build", ".venv", "vendor"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Repo {
    /// Path relative to the scan root, which is what a human recognises.
    pub path: PathBuf,
    /// At least one file in `.git/hooks` dispatches to the binary.
    pub managed: bool,
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
/// `.git/hooks` is somebody's own hook and is never counted as ours — the same
/// test `propagate.sh` used, kept identical on purpose so the two agree.
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

pub fn scan(root: &Path, depth: usize) -> FleetScan {
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
    walk(root, root, depth, &mut s);
    s.repos.sort_by(|a, b| a.path.cmp(&b.path));
    s
}

fn walk(root: &Path, dir: &Path, budget: usize, s: &mut FleetScan) {
    s.dirs_visited += 1;
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
            s.repos.push(Repo {
                path: repo.strip_prefix(root).unwrap_or(repo).to_path_buf(),
                managed,
            });
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
        walk(root, &d, budget - 1, s);
    }
}
