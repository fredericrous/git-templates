//! Which checks apply where.
//!
//! Answers the question the old text output could not: *where does
//! `pre-commit-pyright` actually run?* That one was installed in 6 of 96 repos
//! by historical accident rather than decision, and nothing made that visible.
//!
//! **Applicability here is an approximation, and the UI must never turn it into
//! a verdict.** The hooks themselves scope on the STAGED FILES of a particular
//! commit and on the nearest ancestor manifest; this reasons from manifests at
//! the repo root. A repo with Rust in a subdirectory shows no `rust` and will
//! still run clippy when you touch it. The column answers "would this ever
//! fire here", not "will it fire now".
//!
//! The distinction that must not blur is inert vs failing. `pre-commit-clippy`
//! in a Python repo is CORRECTLY silent; rendering that the same way as a
//! broken hook would manufacture ninety false problems out of the Rust checks
//! alone.

use serde::Serialize;

use githooks_runtime::registry;

use crate::scan::Repo;

/// Which checks apply where.
///
/// Applicability is no longer modelled here. It used to be a `LANGUAGES` table
/// keyed by check name — a fourth copy of information the check already had,
/// and one this module documented as an approximation of what the hooks do.
/// `Scope` on the check is the declaration; the scanner evaluates it against
/// each repository's tracked files and stores the answer.
///
/// The distinction that must not blur is inert vs failing. `pre-commit-clippy`
/// in a Python repo is CORRECTLY silent; rendering that like a broken hook
/// would manufacture ninety false problems out of the Rust checks alone.
/// Every check the dispatcher would run, in dispatcher order.
pub fn all_checks() -> Vec<&'static str> {
    registry::CHECKS.iter().map(|c| c.name).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckRollup {
    pub name: &'static str,
    /// Managed repos where this check could ever fire.
    pub applicable: usize,
    /// Applicable and not suppressed.
    pub active: usize,
    /// Applicable but suppressed by `hook.skip`.
    pub skipped: usize,
    /// Managed repos where it is correctly silent. NOT a problem.
    pub inert: usize,
}

/// `hook.skip` matches by SUBSTRING, exactly as the dispatcher does. Anything
/// else here would report a check as active while the dispatcher skips it.
fn is_skipped(repo: &Repo, check: &str) -> bool {
    repo.skips
        .iter()
        .any(|s| githooks_runtime::skip_suppresses(check, &s.value))
}

pub fn rollup(repos: &[Repo]) -> Vec<CheckRollup> {
    let managed: Vec<&Repo> = repos.iter().filter(|r| r.managed).collect();
    registry::CHECKS
        .iter()
        .map(|check| {
            let (mut applicable, mut skipped, mut inert) = (0, 0, 0);
            for r in &managed {
                if r.applicable.iter().any(|a| a == check.name) {
                    applicable += 1;
                    if is_skipped(r, check.name) {
                        skipped += 1;
                    }
                } else {
                    inert += 1;
                }
            }
            CheckRollup {
                name: check.name,
                applicable,
                active: applicable - skipped,
                skipped,
                inert,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shim::{BakeState, ShimState};
    use std::path::PathBuf;

    /// A repo described by the FILES it contains, because that is what a
    /// check's `Scope` is evaluated against. The fixtures used to be described
    /// by "languages", which was this crate's own model of applicability — the
    /// thing the refactor removed.
    fn repo_with_files(files: &[&str], skips: &[&str], managed: bool) -> Repo {
        let paths: Vec<String> = files.iter().map(|s| s.to_string()).collect();
        // The PRODUCTION filter, not a copy of it: a fixture that recomputes
        // the thing under test keeps passing when the thing under test breaks.
        let applicable = crate::scan::applicable_from_paths(&paths);
        Repo {
            path: PathBuf::from("r"),
            managed,
            shims: vec![ShimState::Ok { baked: "/b".into() }; 4],
            baked: BakeState::Current,
            stale_ours: Vec::new(),
            foreign_subs: Vec::new(),
            hook_pkgjson: false,
            languages: Vec::new(),
            applicable,
            skips: skips.iter().map(|s| crate::skips::for_test(s)).collect(),
            severities: Vec::new(),
            declared: Vec::new(),
        }
    }

    /// Map the old language names onto representative files, so the existing
    /// cases keep asserting the same thing.
    fn repo(langs: &[&str], skips: &[&str], managed: bool) -> Repo {
        let mut files = Vec::new();
        for l in langs {
            match *l {
                "rust" => files.extend(["src/main.rs", "Cargo.toml"]),
                "js" => files.extend(["a.ts", "package.json"]),
                "python" => files.extend(["a.py", "pyproject.toml"]),
                "k8s" => files.extend(["k8s/a.yaml", "kustomization.yaml"]),
                other => panic!("unknown language in fixture: {other}"),
            }
        }
        repo_with_files(&files, skips, managed)
    }

    #[allow(dead_code)]
    fn unused_repo(langs: &[&str], skips: &[&str], managed: bool) -> Repo {
        Repo {
            path: PathBuf::from("r"),
            managed,
            shims: vec![ShimState::Ok { baked: "/b".into() }; 4],
            baked: BakeState::Current,
            stale_ours: Vec::new(),
            foreign_subs: Vec::new(),
            hook_pkgjson: false,
            languages: langs.iter().map(|s| s.to_string()).collect(),
            applicable: Vec::new(),
            skips: skips.iter().map(|s| crate::skips::for_test(s)).collect(),
            severities: Vec::new(),
            declared: Vec::new(),
        }
    }

    fn find<'a>(rs: &'a [CheckRollup], name: &str) -> &'a CheckRollup {
        rs.iter().find(|r| r.name == name).expect("check")
    }

    /// The rule the whole view rests on: a check that cannot fire is INERT, not
    /// broken. Conflating the two would invent 90 false problems from the Rust
    /// checks alone.
    #[test]
    fn a_check_with_no_matching_manifest_is_inert_not_failing() {
        let rs = rollup(&[repo(&["python"], &[], true)]);
        let clippy = find(&rs, "pre-commit-clippy");
        assert_eq!(clippy.applicable, 0);
        assert_eq!(clippy.inert, 1);
        assert_eq!(clippy.active, 0);
        assert_eq!(clippy.skipped, 0, "inert is not skipped");
    }

    #[test]
    fn rows_sum_across() {
        let rs = rollup(&[
            repo(&["rust"], &[], true),
            repo(&["js"], &[], true),
            repo(&["python"], &[], true),
        ]);
        for r in &rs {
            assert_eq!(
                r.applicable + r.inert,
                3,
                "{} must account for every managed repo",
                r.name
            );
            assert_eq!(r.active + r.skipped, r.applicable, "{}", r.name);
        }
    }

    /// `hook.skip` is a substring match in the dispatcher, so it must be one
    /// here too. Exact matching would show a check as active that never runs.
    #[test]
    fn skips_match_by_substring_as_the_dispatcher_does() {
        let rs = rollup(&[repo(&["rust"], &["clippy"], true)]);
        let clippy = find(&rs, "pre-commit-clippy");
        assert_eq!(clippy.applicable, 1);
        assert_eq!(clippy.skipped, 1);
        assert_eq!(clippy.active, 0);
        // A different rust check is untouched by that skip.
        assert_eq!(find(&rs, "pre-commit-cargo-fmt").active, 1);
    }

    #[test]
    fn unmanaged_repos_are_not_counted_at_all() {
        let rs = rollup(&[repo(&["rust"], &[], false)]);
        for r in &rs {
            assert_eq!(r.applicable + r.inert, 0, "{}", r.name);
        }
    }

    /// Only the checks that genuinely have no file condition apply everywhere.
    ///
    /// The old `LANGUAGES` table marked `ban-terms` as `None`, meaning "applies
    /// to every repo" — but it scans `.js/.jsx/.ts/.tsx/.vue` and nothing else,
    /// so the dashboard was reporting it applicable to all 96 repositories.
    /// `lint-json-yaml` and `yamllint` were mis-declared the same way. That is
    /// what a second model of applicability costs: it drifts from the rule the
    /// check actually applies, and nothing notices.
    #[test]
    fn only_genuinely_unconditional_checks_apply_everywhere() {
        let rs = rollup(&[repo(&[], &[], true), repo(&["js"], &[], true)]);
        assert_eq!(
            find(&rs, "pre-push-branch-protect").applicable,
            2,
            "branch-protect has no file condition"
        );
        assert_eq!(
            find(&rs, "pre-commit-merge-conflict").applicable,
            2,
            "nor does merge-conflict"
        );
        assert_eq!(
            find(&rs, "pre-commit-ban-terms").applicable,
            1,
            "ban-terms only scans JS-ish files, whatever the old table said"
        );
    }

    #[test]
    fn the_table_covers_twenty_checks() {
        assert_eq!(all_checks().len(), 20);
        let mut names: Vec<&str> = all_checks();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 20, "duplicate check name");
    }
}
