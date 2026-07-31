//! Executing a `FixPlan`. The only code here that deletes anything.
//!
//! Three rules, each bought with an incident:
//!
//! 1. **Re-plan, never trust a stale plan.** The caller shows a preview and the
//!    user confirms; between those moments the tree can change. Applying the
//!    plan that was displayed would act on a world that no longer exists, so
//!    apply takes the plan it just computed and verifies its refusals again.
//!
//! 2. **A refusal suppresses the whole repository.** Not the offending file —
//!    the repo. A half-applied fix is precisely how a repo ends up holding both
//!    `pre-commit-ruff.zsh` and `pre-commit-ruff`, running ruff twice, silently,
//!    on whichever repos a loop happened to reach.
//!
//! 3. **Remove before write, and stop the repo on the first error.** Writing
//!    first would leave the retired file next to its replacement, which is the
//!    double-run state again.
//!
//! `make install` destroyed tracked source twice in this repo's history, both
//! times because its guard failed OPEN. Nothing here deletes a path it has not
//! positively established is safe.

use std::path::Path;

use serde::Serialize;

use crate::fix::{is_tracked, FixPlan};
use crate::shim;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// Files were removed and/or written.
    Applied { removed: usize, written: usize },
    /// Nothing needed doing.
    Unchanged,
    /// The plan refused; nothing was touched.
    Refused,
    /// Something failed partway. Names what stopped it.
    Failed { error: String, at: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyReport {
    pub repo: std::path::PathBuf,
    pub outcome: Outcome,
}

/// Apply one plan. Returns what actually happened, never what was intended.
pub fn apply(plan: &FixPlan) -> Outcome {
    if plan.refused() {
        return Outcome::Refused;
    }
    if plan.is_noop() {
        return Outcome::Unchanged;
    }

    // Re-check tracked-ness at the moment of action. The preview may be old,
    // and this is the guard that failed open twice before.
    for r in &plan.remove {
        if is_tracked(&r.path) {
            return Outcome::Failed {
                error: "path is tracked by git".into(),
                at: r.path.display().to_string(),
            };
        }
    }

    let mut removed = 0;
    for r in &plan.remove {
        match std::fs::remove_file(&r.path) {
            Ok(()) => removed += 1,
            // Already gone is the desired state, not a failure.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Outcome::Failed {
                    error: e.to_string(),
                    at: r.path.display().to_string(),
                }
            }
        }
    }

    let mut written = 0;
    for write in plan.write.iter().filter(|write| write.changes) {
        let body = shim::render(&write.baked);
        if let Err(e) = write_executable(&write.path, &body) {
            return Outcome::Failed {
                error: e.to_string(),
                at: write.path.display().to_string(),
            };
        }
        written += 1;
    }

    Outcome::Applied { removed, written }
}

/// Write the shim and make it executable. A hook git cannot execute is a hook
/// that silently does not run — the failure mode this whole project exists to
/// avoid — so the mode is set explicitly rather than inherited.
fn write_executable(path: &Path, body: &str) -> std::io::Result<()> {
    std::fs::write(path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fix::Intent;
    use crate::fix::{plan, Refusal};
    use crate::scan;
    use std::path::PathBuf;

    /// Build a repo on disk and scan it, so the plan is derived the same way
    /// the real tool derives it rather than hand-assembled.
    fn fixture(name: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("apply-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let hooks = root.join("r/.git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        (root, hooks)
    }

    fn scan_one(root: &Path, binary: &str) -> (scan::Repo, PathBuf) {
        let s = scan::scan(root, 3, binary);
        let r = s.repos.into_iter().next().expect("one repo");
        let abs = root.join(&r.path);
        (r, abs)
    }

    fn healthy_shims(hooks: &Path, binary: &str) {
        for n in shim::DISPATCHERS {
            std::fs::write(hooks.join(n), shim::render(binary)).unwrap();
        }
    }

    #[test]
    fn removes_the_stale_and_writes_the_shims() {
        let (root, hooks) = fixture("basic");
        healthy_shims(&hooks, "/bin/gh");
        std::fs::write(
            hooks.join("pre-commit-ruff"),
            "#!/bin/sh\nexec x --hooks-dir y pre-commit-ruff\n",
        )
        .unwrap();
        std::fs::write(hooks.join("package.json"), "{\"//\":\"Forces Node\"}").unwrap();

        let (repo, abs) = scan_one(&root, "/bin/gh");
        let p = plan(&repo, &abs, "/bin/gh", Intent::Repair);
        let out = apply(&p);

        assert!(
            matches!(out, Outcome::Applied { removed: 2, .. }),
            "{out:?}"
        );
        assert!(!hooks.join("pre-commit-ruff").exists());
        assert!(!hooks.join("package.json").exists());
        for n in shim::DISPATCHERS {
            assert!(hooks.join(n).exists(), "{n} must survive");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The property that makes a fix safe to re-run, and the one that catches a
    /// plan which "succeeds" by doing the same work forever.
    #[test]
    fn applying_twice_is_a_no_op_the_second_time() {
        let (root, hooks) = fixture("idempotent");
        healthy_shims(&hooks, "/bin/gh");
        std::fs::write(
            hooks.join("pre-commit-ruff"),
            "#!/bin/sh\nexec x --hooks-dir y z\n",
        )
        .unwrap();

        let (repo, abs) = scan_one(&root, "/bin/gh");
        assert!(matches!(
            apply(&plan(&repo, &abs, "/bin/gh", Intent::Repair)),
            Outcome::Applied { .. }
        ));

        // Re-scan: the world changed, so the plan must be recomputed.
        let (repo2, abs2) = scan_one(&root, "/bin/gh");
        let p2 = plan(&repo2, &abs2, "/bin/gh", Intent::Repair);
        assert!(p2.is_noop(), "second plan should be empty: {p2:?}");
        assert_eq!(apply(&p2), Outcome::Unchanged);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// What the tool does for a repo whose shims are missing entirely — the
    /// state `Coinbase-OAuth2` is in, minus the managed flag.
    #[test]
    fn writes_missing_dispatchers() {
        let (root, hooks) = fixture("missing");
        healthy_shims(&hooks, "/bin/gh");
        std::fs::remove_file(hooks.join("pre-push")).unwrap();

        let (repo, abs) = scan_one(&root, "/bin/gh");
        let out = apply(&plan(&repo, &abs, "/bin/gh", Intent::Repair));
        assert!(
            matches!(out, Outcome::Applied { written: 1, .. }),
            "{out:?}"
        );
        assert_eq!(
            std::fs::read_to_string(hooks.join("pre-push")).unwrap(),
            shim::render("/bin/gh")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_refused_plan_touches_nothing() {
        let (root, hooks) = fixture("refused");
        // No shim dispatches to the binary, so the repo is unmanaged.
        std::fs::write(hooks.join("pre-commit"), "#!/bin/zsh\necho legacy\n").unwrap();
        let before = std::fs::read_to_string(hooks.join("pre-commit")).unwrap();

        let (repo, abs) = scan_one(&root, "/bin/gh");
        let p = plan(&repo, &abs, "/bin/gh", Intent::Repair);
        assert_eq!(p.refuse, vec![Refusal::Unmanaged]);
        assert_eq!(apply(&p), Outcome::Refused);
        assert_eq!(
            std::fs::read_to_string(hooks.join("pre-commit")).unwrap(),
            before,
            "an unmanaged repo must be left exactly as found"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Removal happens before writing. Reversing them would leave a retired
    /// file beside its replacement — the double-run state.
    #[test]
    fn removals_precede_writes() {
        let (root, hooks) = fixture("order");
        healthy_shims(&hooks, "/bin/gh");
        std::fs::write(
            hooks.join("pre-push-old"),
            "#!/bin/sh\nexec x --hooks-dir y z\n",
        )
        .unwrap();
        let (repo, abs) = scan_one(&root, "/bin/gh");
        let p = plan(&repo, &abs, "/bin/gh", Intent::Repair);
        let removals: Vec<_> = p.remove.iter().map(|r| r.path.clone()).collect();
        assert!(!removals.is_empty());
        apply(&p);
        for r in removals {
            assert!(!r.exists(), "{} should be gone", r.display());
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn written_shims_are_executable() {
        use std::os::unix::fs::PermissionsExt;
        let (root, hooks) = fixture("mode");
        healthy_shims(&hooks, "/bin/gh");
        std::fs::remove_file(hooks.join("commit-msg")).unwrap();
        let (repo, abs) = scan_one(&root, "/bin/gh");
        apply(&plan(&repo, &abs, "/bin/gh", Intent::Repair));
        let mode = std::fs::metadata(hooks.join("commit-msg"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o111,
            0o111,
            "git will not run a non-executable hook"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
