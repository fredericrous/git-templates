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

use crate::scan::{AgentsMdState, Repo};
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
    /// A dispatcher file exists here and is not one of ours — somebody wrote
    /// their own `pre-commit`. Activating would overwrite it.
    ///
    /// The planner already refused to touch `pre-commit-*` SUB-hooks it did not
    /// recognise; the four dispatchers themselves had no such guard, and
    /// activation is the first mode that writes them into a repository nobody
    /// had installed into. Same failure, one filename along.
    ForeignHook { names: Vec<String> },
    /// `AGENTS.md` carries an unpaired marker. Not routed through `Tracked`:
    /// this file is SUPPOSED to be tracked, so a malformed block is its own
    /// refusal rather than a false positive on that guard.
    AgentsMdMalformed { path: PathBuf },
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

/// Rolling the `AGENTS.md` pointer out. Only ever present when the caller
/// opted in (`plan`'s `agents_md` argument) — writing tracked content across
/// a whole fleet is a materially bigger action than the untracked `.git/hooks`
/// shims `write` covers, so it never happens implicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteAgentsMd {
    pub path: PathBuf,
    /// Always true: this only appears in a plan when the file is missing or
    /// drifted. A repo already at the generated block gets no entry at all,
    /// unlike `WriteShim` — which lists all four dispatchers unconditionally
    /// for parity with `propagate.sh`. There is no such parity target here.
    pub changes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixPlan {
    pub repo: PathBuf,
    pub refuse: Vec<Refusal>,
    pub remove: Vec<Removal>,
    pub write: Vec<WriteShim>,
    pub write_agents_md: Option<WriteAgentsMd>,
}

impl FixPlan {
    /// A plan that would change nothing. Applying twice must produce one of
    /// these the second time.
    pub fn is_noop(&self) -> bool {
        self.remove.is_empty()
            && self.write.iter().all(|w| !w.changes)
            && self.write_agents_md.is_none()
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

/// Why we are planning: repairing an installation, or making one.
///
/// The difference is exactly one refusal. `fix` declines an unmanaged
/// repository because there is nothing there to repair and writing into one
/// would be a decision it was not asked to make. `install` IS that decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Repair,
    Activate,
}

pub fn plan(
    repo: &Repo,
    repo_abs: &Path,
    binary: &str,
    intent: Intent,
    agents_md: bool,
) -> FixPlan {
    let hooks = repo_abs.join(".git/hooks");
    let mut p = FixPlan {
        repo: repo.path.clone(),
        refuse: Vec::new(),
        remove: Vec::new(),
        write: Vec::new(),
        write_agents_md: None,
    };

    if !repo.managed && intent == Intent::Repair {
        p.refuse.push(Refusal::Unmanaged);
        return p;
    }
    // Activating creates the directory; repairing does not, because a missing
    // `.git/hooks` in a repo we thought was managed is a fact worth reporting
    // rather than papering over.
    if intent == Intent::Activate && !hooks.is_dir() {
        let _ = std::fs::create_dir_all(&hooks);
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

    // Only when activating: repairing a managed repo means its dispatchers are
    // already ours, and a drifted one is a repair rather than a stranger.
    if intent == Intent::Activate {
        let foreign: Vec<String> = DISPATCHERS
            .into_iter()
            .filter(|name| {
                std::fs::read_to_string(hooks.join(name))
                    .map(|text| !githooks_runtime::install::is_our_shim(&text))
                    .unwrap_or(false)
            })
            .map(str::to_owned)
            .collect();
        if !foreign.is_empty() {
            p.refuse.push(Refusal::ForeignHook { names: foreign });
            return p;
        }
    }

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

    // Independent of the hook-shim logic above: AGENTS.md is a plain tracked
    // file, not a `.git/hooks` dispatcher, so it is neither routed through
    // `is_tracked` nor suppressed by a hook-shim refusal. Read from the scan's
    // own `repo.agents_md` rather than re-touching the filesystem here —
    // `apply` re-checks fresh at the moment of action, the same split already
    // used for `is_tracked`.
    if agents_md {
        let path = repo_abs.join("AGENTS.md");
        match repo.agents_md {
            AgentsMdState::Missing | AgentsMdState::Drifted => {
                p.write_agents_md = Some(WriteAgentsMd {
                    path,
                    changes: true,
                });
            }
            AgentsMdState::UpToDate => {}
            AgentsMdState::Malformed => {
                p.refuse.push(Refusal::AgentsMdMalformed { path });
            }
        }
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
            declared: Vec::new(),
            trusted: None,
            agents_md: AgentsMdState::Missing,
        }
    }

    #[test]
    fn an_unmanaged_repo_is_refused_not_fixed() {
        let p = plan(
            &repo(false),
            Path::new("/nowhere"),
            "/bin/gh",
            Intent::Repair,
            false,
        );
        assert_eq!(p.refuse, vec![Refusal::Unmanaged]);
        assert!(
            p.remove.is_empty() && p.write.is_empty(),
            "never adopt a repo"
        );
    }

    #[test]
    fn a_missing_hooks_dir_is_refused() {
        let p = plan(
            &repo(true),
            Path::new("/definitely/not/here"),
            "/bin/gh",
            Intent::Repair,
            false,
        );
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

        let p = plan(&r, &dir, "/bin/gh", Intent::Repair, false);
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
        let p = plan(&repo(true), &dir, "/bin/gh", Intent::Repair, false);
        assert!(p.is_noop(), "{p:?}");
        assert_eq!(p.write.len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A repo with no `.git/hooks`, otherwise healthy, so only the
    /// `agents_md` argument is under test.
    fn healthy_repo_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fixplan-agents-md-{name}-{}", std::process::id()));
        let hooks = dir.join(".git/hooks");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&hooks).unwrap();
        for n in DISPATCHERS {
            std::fs::write(hooks.join(n), shim::render("/bin/gh")).unwrap();
        }
        dir
    }

    #[test]
    fn agents_md_is_never_planned_without_opting_in() {
        let dir = healthy_repo_dir("optout");
        let mut r = repo(true);
        r.agents_md = AgentsMdState::Missing;
        let p = plan(&r, &dir, "/bin/gh", Intent::Repair, false);
        assert!(p.write_agents_md.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_drifted_agents_md_is_planned_when_opted_in() {
        for state in [AgentsMdState::Missing, AgentsMdState::Drifted] {
            let dir = healthy_repo_dir("plan");
            let mut r = repo(true);
            r.agents_md = state;
            let p = plan(&r, &dir, "/bin/gh", Intent::Repair, true);
            assert!(!p.is_noop(), "{p:?}");
            let w = p.write_agents_md.expect("must plan a write");
            assert!(w.changes);
            assert_eq!(w.path, dir.join("AGENTS.md"));
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn an_up_to_date_agents_md_is_not_replanned() {
        let dir = healthy_repo_dir("uptodate");
        let mut r = repo(true);
        r.agents_md = AgentsMdState::UpToDate;
        let p = plan(&r, &dir, "/bin/gh", Intent::Repair, true);
        assert!(p.write_agents_md.is_none());
        assert!(p.is_noop(), "{p:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_malformed_agents_md_refuses_the_repo() {
        let dir = healthy_repo_dir("malformed");
        let mut r = repo(true);
        r.agents_md = AgentsMdState::Malformed;
        let p = plan(&r, &dir, "/bin/gh", Intent::Repair, true);
        assert_eq!(
            p.refuse,
            vec![Refusal::AgentsMdMalformed {
                path: dir.join("AGENTS.md")
            }]
        );
        assert!(p.write_agents_md.is_none());
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
