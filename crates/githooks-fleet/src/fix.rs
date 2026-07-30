//! What it would take to bring one repository back to the shipped state.
//!
//! A plan, never an action. PR 5 executes exactly this structure; nothing here
//! touches the filesystem, which is what makes the parity differential against
//! `propagate.sh` meaningful — two implementations can be compared before
//! either is trusted to delete anything.
//!
//! The refusals are the important half. `make install` destroyed tracked source
//! twice in this repo's history, both times because its guard FAILED OPEN:
//! anything it could not confirm became safe to delete. So a plan refuses on
//! anything it cannot positively establish, and a refusal suppresses the whole
//! repo rather than the individual file — a half-applied fix is how a repo ends
//! up with both `pre-commit-ruff.zsh` and `pre-commit-ruff`, running ruff twice.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::scan::Repo;
use crate::shim::{self, DISPATCHERS};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "refusal", rename_all = "snake_case")]
pub enum Refusal {
    /// No shim dispatches to the binary. Either somebody else's repo or, as
    /// `propagate.sh` also decided, an application's data repo that commits
    /// programmatically and must never gain hooks.
    Unmanaged,
    /// The hooks directory could not be read. Absence of evidence, not evidence
    /// of absence.
    UnreadableHooks,
    /// git tracks this path. An install step must never delete tracked source,
    /// whatever the path resolution says.
    Tracked { path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalReason {
    /// Ours, but no longer shipped — the 16 per-check shims retired when checks
    /// moved in-process.
    StaleOurs,
    /// A hand-written sub-hook. Nothing dispatches these since the move
    /// in-process, so it looks installed and never runs.
    ForeignSubHook,
    /// The node-era `package.json` that forced CommonJS. No hook is node now.
    VestigialPackageJson,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Removal {
    pub path: PathBuf,
    pub reason: RemovalReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteShim {
    pub path: PathBuf,
    pub baked: String,
    /// False when the file already has exactly these bytes. The write is listed
    /// either way, because `propagate.sh` writes all four unconditionally and
    /// the parity gate compares the SET of writes — but the UI should be able
    /// to say "4 writes, 0 of them changes".
    pub changes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixPlan {
    pub repo: PathBuf,
    pub refuse: Vec<Refusal>,
    pub remove: Vec<Removal>,
    pub write: Vec<WriteShim>,
}

impl FixPlan {
    /// A plan that would change nothing. Applying twice must produce one of
    /// these the second time.
    pub fn is_noop(&self) -> bool {
        self.remove.is_empty() && self.write.iter().all(|w| !w.changes)
    }
    pub fn refused(&self) -> bool {
        !self.refuse.is_empty()
    }
}

/// Does git track this path? Consulted before any removal.
///
/// Files under `.git/hooks` are never tracked, so in the normal case this is
/// pure defence. It exists because the normal case is not the one that broke:
/// `~/.config/git/git-templates` is a SYMLINK to a checkout, and an install
/// step that resolved through it deleted tracked templates twice.
pub fn is_tracked(path: &Path) -> bool {
    let Some(dir) = path.parent() else {
        return false;
    };
    Command::new("git")
        .args(["ls-files", "--error-unmatch"])
        .arg(path)
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn plan(repo: &Repo, repo_abs: &Path, binary: &str) -> FixPlan {
    let hooks = repo_abs.join(".git/hooks");
    let mut p = FixPlan {
        repo: repo.path.clone(),
        refuse: Vec::new(),
        remove: Vec::new(),
        write: Vec::new(),
    };

    if !repo.managed {
        p.refuse.push(Refusal::Unmanaged);
        return p;
    }
    if !hooks.is_dir() {
        p.refuse.push(Refusal::UnreadableHooks);
        return p;
    }

    for name in &repo.stale_ours {
        p.remove.push(Removal {
            path: hooks.join(name),
            reason: RemovalReason::StaleOurs,
        });
    }
    for name in &repo.foreign_subs {
        p.remove.push(Removal {
            path: hooks.join(name),
            reason: RemovalReason::ForeignSubHook,
        });
    }
    if repo.hook_pkgjson {
        p.remove.push(Removal {
            path: hooks.join("package.json"),
            reason: RemovalReason::VestigialPackageJson,
        });
    }
    p.remove.sort_by(|a, b| a.path.cmp(&b.path));

    let rendered = shim::render(binary);
    for name in DISPATCHERS {
        let path = hooks.join(name);
        let changes = std::fs::read_to_string(&path)
            .map(|c| c != rendered)
            .unwrap_or(true);
        p.write.push(WriteShim {
            path,
            baked: binary.to_string(),
            changes,
        });
    }

    // Fail closed: one tracked path suppresses the WHOLE repo, because a
    // partially applied plan is how a repo ends up running a check twice.
    let tracked: Vec<PathBuf> = p
        .remove
        .iter()
        .map(|r| r.path.clone())
        .chain(p.write.iter().map(|w| w.path.clone()))
        .filter(|path| is_tracked(path))
        .collect();
    if !tracked.is_empty() {
        p.refuse
            .extend(tracked.into_iter().map(|path| Refusal::Tracked { path }));
        p.remove.clear();
        p.write.clear();
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shim::{BakeState, ShimState};

    fn repo(managed: bool) -> Repo {
        Repo {
            path: PathBuf::from("r"),
            managed,
            shims: vec![ShimState::Missing; 4],
            baked: BakeState::None,
            stale_ours: Vec::new(),
            foreign_subs: Vec::new(),
            hook_pkgjson: false,
            languages: Vec::new(),
            applicable: Vec::new(),
            skips: Vec::new(),
            severities: Vec::new(),
        }
    }

    #[test]
    fn an_unmanaged_repo_is_refused_not_fixed() {
        let p = plan(&repo(false), Path::new("/nowhere"), "/bin/gh");
        assert_eq!(p.refuse, vec![Refusal::Unmanaged]);
        assert!(
            p.remove.is_empty() && p.write.is_empty(),
            "never adopt a repo"
        );
    }

    #[test]
    fn a_missing_hooks_dir_is_refused() {
        let p = plan(&repo(true), Path::new("/definitely/not/here"), "/bin/gh");
        assert_eq!(p.refuse, vec![Refusal::UnreadableHooks]);
    }

    #[test]
    fn removals_are_classified_and_sorted() {
        let dir = std::env::temp_dir().join(format!("fixplan-{}", std::process::id()));
        let hooks = dir.join(".git/hooks");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&hooks).unwrap();

        let mut r = repo(true);
        r.stale_ours = vec!["pre-commit-ruff".into()];
        r.foreign_subs = vec!["pre-push-mine.sh".into()];
        r.hook_pkgjson = true;

        let p = plan(&r, &dir, "/bin/gh");
        assert_eq!(p.remove.len(), 3);
        let reasons: Vec<_> = p.remove.iter().map(|r| r.reason).collect();
        assert!(reasons.contains(&RemovalReason::StaleOurs));
        assert!(reasons.contains(&RemovalReason::ForeignSubHook));
        assert!(reasons.contains(&RemovalReason::VestigialPackageJson));
        assert_eq!(
            p.write.len(),
            4,
            "all four are written, as propagate.sh does"
        );
        assert!(p.write.iter().all(|w| w.changes), "none exist yet");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Writing bytes that are already there is listed but marked as no change,
    /// so "fix" on a healthy repo is visibly a no-op.
    #[test]
    fn an_already_correct_repo_plans_no_changes() {
        let dir = std::env::temp_dir().join(format!("fixplan-ok-{}", std::process::id()));
        let hooks = dir.join(".git/hooks");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&hooks).unwrap();
        for n in DISPATCHERS {
            std::fs::write(hooks.join(n), shim::render("/bin/gh")).unwrap();
        }
        let p = plan(&repo(true), &dir, "/bin/gh");
        assert!(p.is_noop(), "{p:?}");
        assert_eq!(p.write.len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The predicate that keeps the apply path away from tracked source. Proven
    /// against a real repository rather than mocked, because the whole point is
    /// that it agrees with git.
    #[test]
    fn is_tracked_agrees_with_git() {
        let dir = std::env::temp_dir().join(format!("fixplan-tracked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git");
        };
        git(&["init", "-q", "--template=", "."]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        std::fs::write(dir.join("tracked.txt"), "x").unwrap();
        std::fs::write(dir.join("untracked.txt"), "x").unwrap();
        git(&["add", "tracked.txt"]);
        git(&["commit", "-q", "--no-verify", "-m", "chore: seed"]);

        assert!(is_tracked(&dir.join("tracked.txt")));
        assert!(!is_tracked(&dir.join("untracked.txt")));
        assert!(!is_tracked(&dir.join("does-not-exist.txt")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
