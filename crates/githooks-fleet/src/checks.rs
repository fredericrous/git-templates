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

use crate::scan::Repo;

/// A check, and what makes it relevant to a repository.
pub struct Check {
    pub name: &'static str,
    /// `None` — applies to every managed repo, because the check scopes on the
    /// staged files rather than on any manifest.
    pub language: Option<&'static str>,
}

pub const CHECKS: &[Check] = &[
    // pre-commit
    Check {
        name: "pre-commit-ban-terms",
        language: None,
    },
    Check {
        name: "pre-commit-merge-conflict",
        language: None,
    },
    Check {
        name: "pre-commit-package-lock",
        language: Some("js"),
    },
    Check {
        name: "pre-commit-usual-name",
        language: None,
    },
    Check {
        name: "pre-commit-lint-json-yaml",
        language: None,
    },
    Check {
        name: "pre-commit-yamllint",
        language: None,
    },
    Check {
        name: "pre-commit-lint-js",
        language: Some("js"),
    },
    Check {
        name: "pre-commit-prettier",
        language: Some("js"),
    },
    Check {
        name: "pre-commit-ruff",
        language: Some("python"),
    },
    Check {
        name: "pre-commit-pyright",
        language: Some("python"),
    },
    Check {
        name: "pre-commit-cargo-fmt",
        language: Some("rust"),
    },
    Check {
        name: "pre-commit-clippy",
        language: Some("rust"),
    },
    Check {
        name: "pre-commit-argo-lint",
        language: Some("k8s"),
    },
    Check {
        name: "pre-commit-kube-linter",
        language: Some("k8s"),
    },
    Check {
        name: "pre-commit-kubeconform",
        language: Some("k8s"),
    },
    // pre-push
    Check {
        name: "pre-push-branch-protect",
        language: None,
    },
    Check {
        name: "pre-push-branch-pattern",
        language: None,
    },
    Check {
        name: "pre-push-pull-rebase",
        language: None,
    },
    Check {
        name: "pre-push-run-tests-js",
        language: Some("js"),
    },
    Check {
        name: "pre-push-cargo-test",
        language: Some("rust"),
    },
];

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
    repo.skips.iter().any(|s| check.contains(s.as_str()))
}

fn applies(repo: &Repo, check: &Check) -> bool {
    match check.language {
        None => true,
        Some(lang) => repo.languages.iter().any(|l| l == lang),
    }
}

pub fn rollup(repos: &[Repo]) -> Vec<CheckRollup> {
    let managed: Vec<&Repo> = repos.iter().filter(|r| r.managed).collect();
    CHECKS
        .iter()
        .map(|c| {
            let (mut applicable, mut skipped, mut inert) = (0, 0, 0);
            for r in &managed {
                if applies(r, c) {
                    applicable += 1;
                    if is_skipped(r, c.name) {
                        skipped += 1;
                    }
                } else {
                    inert += 1;
                }
            }
            CheckRollup {
                name: c.name,
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
            skips: skips.iter().map(|s| s.to_string()).collect(),
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
    #[test]
    fn the_table_covers_twenty_checks() {
        assert_eq!(CHECKS.len(), 20);
        let mut names: Vec<&str> = CHECKS.iter().map(|c| c.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 20, "duplicate check name");
    }
}
