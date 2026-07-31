//! The hook registry — one table, one signature.
//!
//! Before this, dispatch was a 20-arm `match` in main.rs and handlers had four
//! different signatures (`run(&args)`, `run(&hook, &args)`, `argo_lint(&args)`,
//! …). Two costs, one of them real:
//!
//!   - adding a hook meant touching a match arm, a module, and remembering
//!     which signature that one used;
//!   - the hook NAME was written twice — in the arm and as the shim's filename
//!     — with nothing checking they agree. A shim the binary does not recognise
//!     exits 2 and blocks the commit; a handler with no shim is dead code. The
//!     consistency test below turns that pairing into something enforced.

use std::ffi::OsString;
use std::path::Path;

use crate::check::{Builtin, Outcome, Scope, Severity, Stage};
use crate::pushrefs::PushRefs;
use crate::{dispatch, hooks};

/// Everything a hook is given. One shape for all of them, so a handler that
/// needs the invoked name (ban-terms excludes its own source by it) or the
/// hooks directory (the dispatchers glob it) does not need its own signature.
pub struct Ctx<'a> {
    /// The hook name as invoked.
    pub name: &'a str,
    /// Arguments git passed the hook.
    pub args: &'a [OsString],
    /// Directory the shim lives in. Only foreign sub-hooks are found here now;
    /// our own checks are functions in this binary.
    pub hooks_dir: &'a Path,
    /// The pre-push ref list, read from stdin at most once and lent to every
    /// check that asks. See `pushrefs`.
    pub push: &'a PushRefs,
}

pub type HookFn = fn(&Ctx) -> i32;

/// name → handler. The single place a hook is registered.
/// The four hook names git itself invokes. Everything else is a `Check`.
pub const ENTRYPOINTS: &[(&str, HookFn)] = &[
    ("pre-commit", dispatch::pre_commit),
    ("pre-push", dispatch::pre_push),
    ("commit-msg", |c| hooks::commit_msg::run(c.args)),
    ("prepare-commit-msg", |c| {
        hooks::prepare_commit_msg::run(c.args)
    }),
];

/// Every check, in the order its stage runs them.
///
/// ONE declaration each: name, stage, scope and function together. This
/// replaced `REGISTRY` plus `PRE_COMMIT_CHECKS` plus `PRE_PUSH_CHECKS` plus the
/// fleet crate's `LANGUAGES` — four tables keyed by the same string, kept in
/// step by reconciliation tests that are now unnecessary rather than passing.
///
/// pre-push order is the cost order: refuse a forbidden push before validating
/// a name, and validate everything structural before paying for a test suite.
pub const CHECKS: &[Builtin] = &[
    // ---- pre-commit ----
    Builtin {
        name: "pre-commit-argo-lint",
        stage: Stage::PreCommit,
        scope: Scope::new(
            &[".yaml", ".yml"],
            &["kustomization.yaml", "kustomization.yml"],
        ),
        severity: Severity::Block,
        run: |c| hooks::k8s::argo_lint(c.args),
    },
    Builtin {
        name: "pre-commit-ban-terms",
        stage: Stage::PreCommit,
        scope: Scope::files(&[".js", ".jsx", ".ts", ".tsx", ".vue"]),
        severity: Severity::Block,
        run: |c| Outcome::from_code(hooks::ban_terms::run(c.name, c.args)),
    },
    Builtin {
        name: "pre-commit-cargo-fmt",
        stage: Stage::PreCommit,
        scope: Scope::new(&[".rs"], &["Cargo.toml"]),
        severity: Severity::Block,
        run: |c| hooks::rust_tools::fmt(c.args),
    },
    Builtin {
        name: "pre-commit-clippy",
        stage: Stage::PreCommit,
        scope: Scope::new(&[".rs"], &["Cargo.toml"]),
        severity: Severity::Block,
        run: |c| hooks::rust_tools::clippy(c.args),
    },
    Builtin {
        name: "pre-commit-kube-linter",
        stage: Stage::PreCommit,
        scope: Scope::new(
            &[".yaml", ".yml"],
            &[".kube-linter*.yaml", ".kube-linter*.yml"],
        ),
        severity: Severity::Block,
        run: |c| hooks::k8s::kube_linter(c.args),
    },
    Builtin {
        name: "pre-commit-kubeconform",
        stage: Stage::PreCommit,
        scope: Scope::new(
            &[".yaml", ".yml"],
            &["kustomization.yaml", "kustomization.yml"],
        ),
        severity: Severity::Block,
        run: |c| hooks::k8s::kubeconform(c.args),
    },
    Builtin {
        name: "pre-commit-lint-js",
        stage: Stage::PreCommit,
        scope: Scope::new(&[".js", ".jsx", ".ts", ".tsx", ".vue"], &["package.json"]),
        severity: Severity::Block,
        run: |c| hooks::lint_js::run(c.args),
    },
    Builtin {
        name: "pre-commit-lint-json-yaml",
        stage: Stage::PreCommit,
        scope: Scope::files(&[".json", ".yaml", ".yml"]),
        severity: Severity::Block,
        run: |c| hooks::lint_json_yaml::run(c.args),
    },
    Builtin {
        name: "pre-commit-merge-conflict",
        stage: Stage::PreCommit,
        scope: Scope::ALWAYS,
        severity: Severity::Block,
        run: |c| Outcome::from_code(hooks::merge_conflict::run(c.name, c.args)),
    },
    Builtin {
        name: "pre-commit-package-lock",
        stage: Stage::PreCommit,
        scope: Scope::new(&[], &["package.json"]),
        severity: Severity::Block,
        run: |c| hooks::package_lock::run(c.args),
    },
    Builtin {
        name: "pre-commit-prettier",
        stage: Stage::PreCommit,
        scope: Scope::new(
            &[],
            &[
                ".prettierrc",
                ".prettierrc.json",
                ".prettierrc.yml",
                ".prettierrc.yaml",
                ".prettierrc.js",
                "prettier.config.js",
            ],
        ),
        severity: Severity::Block,
        run: |c| hooks::prettier::run(c.args),
    },
    Builtin {
        name: "pre-commit-pyright",
        stage: Stage::PreCommit,
        scope: Scope::new(
            &[".py", ".pyi"],
            &[
                "pyrightconfig.json",
                "pyrightconfig.jsonc",
                "pyproject.toml",
            ],
        ),
        severity: Severity::Block,
        run: |c| hooks::python_tools::pyright(c.args),
    },
    Builtin {
        name: "pre-commit-ruff",
        stage: Stage::PreCommit,
        scope: Scope::new(
            &[".py", ".pyi"],
            &["ruff.toml", ".ruff.toml", "pyproject.toml"],
        ),
        severity: Severity::Block,
        run: |c| hooks::python_tools::ruff(c.args),
    },
    Builtin {
        name: "pre-commit-usual-name",
        stage: Stage::PreCommit,
        scope: Scope::ALWAYS,
        severity: Severity::Block,
        run: |c| hooks::usual_name::run(c.args),
    },
    Builtin {
        name: "pre-commit-yamllint",
        stage: Stage::PreCommit,
        scope: Scope::new(
            &[".yaml", ".yml"],
            &[".yamllint.yaml", ".yamllint.yml", ".yamllint"],
        ),
        severity: Severity::Block,
        run: |c| hooks::yamllint::run(c.args),
    },
    // ---- pre-push, cheapest and most decisive first ----
    Builtin {
        name: "pre-push-branch-protect",
        stage: Stage::PrePush,
        scope: Scope::ALWAYS,
        severity: Severity::Block,
        run: |c| Outcome::from_code(hooks::branch_protect::run(c.push.get())),
    },
    Builtin {
        name: "pre-push-branch-pattern",
        stage: Stage::PrePush,
        scope: Scope::ALWAYS,
        severity: Severity::Block,
        run: |c| Outcome::from_code(hooks::branch_pattern::run(c.args)),
    },
    Builtin {
        name: "pre-push-pull-rebase",
        stage: Stage::PrePush,
        scope: Scope::ALWAYS,
        severity: Severity::Block,
        run: |c| Outcome::from_code(hooks::pull_rebase::run(c.args)),
    },
    Builtin {
        name: "pre-push-run-tests-js",
        stage: Stage::PrePush,
        scope: Scope::new(&[".js", ".jsx", ".ts", ".tsx", ".vue"], &["package.json"]),
        severity: Severity::Block,
        run: |c| Outcome::from_code(hooks::run_tests::run(c.push.get())),
    },
    Builtin {
        name: "pre-push-cargo-test",
        stage: Stage::PrePush,
        scope: Scope::new(&[".rs"], &["Cargo.toml"]),
        severity: Severity::Block,
        run: |c| hooks::rust_tools::test(c.push.get()),
    },
];

/// A check's severity, after any per-repository override.
///
/// `git config githooks.severity.<check> warn` downgrades a blocking check to a
/// warning. Unlike `hook.skip` it keeps the signal: the check still runs and
/// still reports, it just stops failing the commit.
pub fn severity_of(check: &Builtin) -> Severity {
    let key = format!("githooks.severity.{}", check.name);
    match crate::git::stdout(&["config", "--get", &key]).as_deref() {
        Some("warn") => Severity::Warn,
        Some("block") => Severity::Block,
        _ => check.severity,
    }
}

/// Checks for one stage, in declared order.
pub fn stage_checks(stage: Stage) -> impl Iterator<Item = &'static Builtin> {
    CHECKS.iter().filter(move |c| c.stage == stage)
}

pub fn lookup(name: &str) -> Option<HookFn> {
    if let Some((_, f)) = ENTRYPOINTS.iter().find(|(n, _)| *n == name) {
        return Some(*f);
    }
    // A check invoked directly by name — how the tests drive individual checks,
    // and how `githooks <check>` works from a shell. Its Outcome collapses to an
    // exit code here, honouring severity, so a `warn` check invoked directly
    // reports without failing exactly as it does inside a dispatcher.
    if CHECKS.iter().any(|c| c.name == name) {
        return Some(|c: &Ctx| {
            let check = CHECKS
                .iter()
                .find(|k| k.name == c.name)
                .expect("checked above");
            match (check.run)(c) {
                Outcome::Failed if severity_of(check) == Severity::Block => 1,
                _ => 0,
            }
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{lookup, Stage, CHECKS, ENTRYPOINTS};
    use std::collections::BTreeSet;

    #[test]
    fn names_are_unique_across_entrypoints_and_checks() {
        let mut seen = BTreeSet::new();
        for n in ENTRYPOINTS
            .iter()
            .map(|(n, _)| *n)
            .chain(CHECKS.iter().map(|c| c.name))
        {
            assert!(seen.insert(n), "duplicate registration: {n}");
        }
    }

    /// Only FOUR files ship, and they are exactly the hook names git invokes.
    #[test]
    fn the_shipped_shims_are_exactly_the_git_invoked_hooks() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates/hooks");
        let mut shipped: Vec<String> = std::fs::read_dir(dir)
            .expect("templates/hooks")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        shipped.sort();
        assert_eq!(
            shipped,
            vec!["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"]
        );
        for name in &shipped {
            assert!(
                lookup(name).is_some(),
                "shipped shim {name:?} has no handler"
            );
        }
    }

    /// Every check is reachable by name, which is how a shell — and the tests —
    /// invoke one directly.
    #[test]
    fn every_check_is_reachable_by_name() {
        for c in CHECKS {
            assert!(lookup(c.name).is_some(), "{} not reachable", c.name);
        }
        assert!(lookup("pre-commit-not-a-check").is_none());
    }

    /// pre-push is serial and fail-fast, so declaration order IS cost order.
    #[test]
    fn pre_push_runs_cheapest_first() {
        let order: Vec<&str> = super::stage_checks(Stage::PrePush)
            .map(|c| c.name)
            .collect();
        assert_eq!(
            order,
            vec![
                "pre-push-branch-protect",
                "pre-push-branch-pattern",
                "pre-push-pull-rebase",
                "pre-push-run-tests-js",
                "pre-push-cargo-test",
            ]
        );
    }

    /// The reconciliation tests that used to live here are gone, and that is
    /// the point of the refactor: there is no second table to disagree with.
    #[test]
    fn every_check_declares_a_stage_and_a_scope() {
        assert_eq!(CHECKS.len(), 20);
        let pre_commit = super::stage_checks(Stage::PreCommit).count();
        let pre_push = super::stage_checks(Stage::PrePush).count();
        assert_eq!(
            pre_commit + pre_push,
            CHECKS.len(),
            "every check has a stage"
        );
    }
}
