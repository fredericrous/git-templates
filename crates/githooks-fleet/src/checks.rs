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

/// What makes a check relevant to a repository.
///
/// The NAMES come from the registry — `githooks_runtime::registry` — rather
/// than a copy. They were hand-copied here at first, and agreed with the
/// dispatcher only by luck: adding a check to the registry would have left the
/// dashboard silently omitting it from every view and under-reporting blast
/// radius. Only the language mapping belongs to this crate, because scoping is
/// a display concern the dispatcher has no opinion about.
///
/// `every_check_has_a_language_decision` fails if a new check appears without
/// one, so the choice is forced at the moment it is introduced rather than
/// defaulted silently — the same reconciliation the commit-type and
/// branch-prefix vocabularies use.
const LANGUAGES: &[(&str, Option<&str>)] = &[
    ("pre-commit-ban-terms", None),
    ("pre-commit-merge-conflict", None),
    ("pre-commit-package-lock", Some("js")),
    ("pre-commit-usual-name", None),
    ("pre-commit-lint-json-yaml", None),
    ("pre-commit-yamllint", None),
    ("pre-commit-lint-js", Some("js")),
    ("pre-commit-prettier", Some("js")),
    ("pre-commit-ruff", Some("python")),
    ("pre-commit-pyright", Some("python")),
    ("pre-commit-cargo-fmt", Some("rust")),
    ("pre-commit-clippy", Some("rust")),
    ("pre-commit-argo-lint", Some("k8s")),
    ("pre-commit-kube-linter", Some("k8s")),
    ("pre-commit-kubeconform", Some("k8s")),
    ("pre-push-branch-protect", None),
    ("pre-push-branch-pattern", None),
    ("pre-push-pull-rebase", None),
    ("pre-push-run-tests-js", Some("js")),
    ("pre-push-cargo-test", Some("rust")),
];

/// Every check the dispatcher would run, in dispatcher order.
pub fn all_checks() -> Vec<&'static str> {
    registry::PRE_COMMIT_CHECKS
        .iter()
        .chain(registry::PRE_PUSH_CHECKS.iter())
        .copied()
        .collect()
}

fn language_of(check: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .find(|(n, _)| *n == check)
        .and_then(|(_, l)| *l)
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
    all_checks()
        .into_iter()
        .map(|name| {
            let lang = language_of(name);
            let (mut applicable, mut skipped, mut inert) = (0, 0, 0);
            for r in &managed {
                let applies = match lang {
                    None => true,
                    Some(l) => r.languages.iter().any(|x| x == l),
                };
                if applies {
                    applicable += 1;
                    if is_skipped(r, name) {
                        skipped += 1;
                    }
                } else {
                    inert += 1;
                }
            }
            CheckRollup {
                name,
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

    fn repo(langs: &[&str], skips: &[&str], managed: bool) -> Repo {
        Repo {
            path: PathBuf::from("r"),
            managed,
            shims: vec![ShimState::Ok { baked: "/b".into() }; 4],
            baked: BakeState::Current,
            stale_ours: Vec::new(),
            foreign_subs: Vec::new(),
            hook_pkgjson: false,
            languages: langs.iter().map(|s| s.to_string()).collect(),
            skips: skips.iter().map(|s| crate::skips::for_test(s)).collect(),
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

    #[test]
    fn language_free_checks_apply_everywhere() {
        let rs = rollup(&[repo(&[], &[], true), repo(&["js"], &[], true)]);
        assert_eq!(find(&rs, "pre-commit-ban-terms").applicable, 2);
        assert_eq!(find(&rs, "pre-push-branch-protect").applicable, 2);
    }

    /// Every registered check is listed. A check missing from this table would
    /// silently never appear in the view.
    /// Adding a check to the registry must force a language decision here,
    /// rather than defaulting it silently to "applies everywhere". Same
    /// reconciliation the commit-type and branch-prefix vocabularies use: make
    /// the omission fail at the moment it is introduced.
    #[test]
    fn every_check_has_a_language_decision() {
        let mapped: std::collections::BTreeSet<&str> = LANGUAGES.iter().map(|(n, _)| *n).collect();
        let missing: Vec<&str> = all_checks()
            .into_iter()
            .filter(|c| !mapped.contains(c))
            .collect();
        assert!(
            missing.is_empty(),
            "registered checks with no language decision: {missing:?}"
        );
        let orphan: Vec<&str> = mapped
            .iter()
            .copied()
            .filter(|n| !all_checks().contains(n))
            .collect();
        assert!(
            orphan.is_empty(),
            "language decisions for dead checks: {orphan:?}"
        );
    }

    /// And the names come from the registry, not a copy.
    #[test]
    fn the_check_list_is_the_registrys() {
        // NAMES, not a count: a hand-copied table of the same length would
        // satisfy a length check while omitting the check that was just added.
        let expected: Vec<&str> = registry::PRE_COMMIT_CHECKS
            .iter()
            .chain(registry::PRE_PUSH_CHECKS.iter())
            .copied()
            .collect();
        assert_eq!(all_checks(), expected);
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
