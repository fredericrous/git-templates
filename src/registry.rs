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
pub const REGISTRY: &[(&str, HookFn)] = &[
    ("pre-commit", dispatch::pre_commit),
    ("pre-push", dispatch::pre_push),
    ("commit-msg", |c| hooks::commit_msg::run(c.args)),
    ("prepare-commit-msg", |c| {
        hooks::prepare_commit_msg::run(c.args)
    }),
    ("pre-commit-argo-lint", |c| hooks::k8s::argo_lint(c.args)),
    ("pre-commit-ban-terms", |c| {
        hooks::ban_terms::run(c.name, c.args)
    }),
    ("pre-commit-kube-linter", |c| {
        hooks::k8s::kube_linter(c.args)
    }),
    ("pre-commit-kubeconform", |c| {
        hooks::k8s::kubeconform(c.args)
    }),
    ("pre-commit-lint-js", |c| hooks::lint_js::run(c.args)),
    ("pre-commit-lint-json-yaml", |c| {
        hooks::lint_json_yaml::run(c.args)
    }),
    ("pre-commit-merge-conflict", |c| {
        hooks::merge_conflict::run(c.name, c.args)
    }),
    ("pre-commit-package-lock", |c| {
        hooks::package_lock::run(c.args)
    }),
    ("pre-commit-prettier", |c| hooks::prettier::run(c.args)),
    ("pre-commit-pyright", |c| {
        hooks::python_tools::pyright(c.args)
    }),
    ("pre-commit-ruff", |c| hooks::python_tools::ruff(c.args)),
    ("pre-commit-usual-name", |c| hooks::usual_name::run(c.args)),
    ("pre-commit-yamllint", |c| hooks::yamllint::run(c.args)),
    ("pre-push-branch-pattern", |c| {
        hooks::branch_pattern::run(c.args)
    }),
    ("pre-push-pull-rebase", |c| hooks::pull_rebase::run(c.args)),
    ("pre-push-run-tests-js", |c| {
        hooks::run_tests::run(c.push.get())
    }),
    ("pre-push-branch-protect", |c| {
        hooks::branch_protect::run(c.push.get())
    }),
];

/// What `pre-commit` runs, in the order it reports them. This list REPLACES the
/// old `<hook>-*` filename glob: order used to be lexicographic and therefore
/// an accident of naming, which is how a rename could silently reorder a gate.
/// Here it is stated.
pub const PRE_COMMIT_CHECKS: &[&str] = &[
    "pre-commit-argo-lint",
    "pre-commit-ban-terms",
    "pre-commit-kube-linter",
    "pre-commit-kubeconform",
    "pre-commit-lint-js",
    "pre-commit-lint-json-yaml",
    "pre-commit-merge-conflict",
    "pre-commit-package-lock",
    "pre-commit-prettier",
    "pre-commit-pyright",
    "pre-commit-ruff",
    "pre-commit-usual-name",
    "pre-commit-yamllint",
];

/// What `pre-push` runs, in order. Serial and fail-fast, so this order is the
/// cost order: refuse a forbidden push before validating a name, and validate
/// everything structural before paying for the test suite.
pub const PRE_PUSH_CHECKS: &[&str] = &[
    "pre-push-branch-protect",
    "pre-push-branch-pattern",
    "pre-push-pull-rebase",
    "pre-push-run-tests-js",
];

pub fn lookup(name: &str) -> Option<HookFn> {
    REGISTRY.iter().find(|(n, _)| *n == name).map(|(_, f)| *f)
}

#[cfg(test)]
mod tests {
    use super::{PRE_COMMIT_CHECKS, PRE_PUSH_CHECKS, REGISTRY};
    use std::collections::BTreeSet;

    /// Only FOUR files ship now, and they are exactly the hook names git itself
    /// invokes. Everything else `.git/hooks` used to hold was our own
    /// dispatcher's business — 16 identical shims whose only job was to re-exec
    /// this binary and tell it its own name.
    #[test]
    fn the_shipped_shims_are_exactly_the_git_invoked_hooks() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/templates/hooks");
        let mut shipped: Vec<String> = std::fs::read_dir(dir)
            .expect("templates/hooks")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        shipped.sort();
        assert_eq!(
            shipped,
            vec!["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"],
            "git invokes these four by name; anything else here is a file we \
             would have to install into 96 repos for no reason"
        );
        for name in &shipped {
            assert!(
                super::lookup(name).is_some(),
                "shipped shim {name:?} has no handler — it would exit 2 and block the commit"
            );
        }
    }

    /// The replacement for the old shim↔handler pairing: a check that is
    /// registered but named in NEITHER list can never run. That used to be
    /// caught by "handler with no shim is dead code"; with the shims gone, the
    /// check lists are what reachability means.
    #[test]
    fn every_check_is_reachable_and_every_listed_check_exists() {
        let listed: BTreeSet<&str> = PRE_COMMIT_CHECKS
            .iter()
            .chain(PRE_PUSH_CHECKS.iter())
            .copied()
            .collect();
        for n in &listed {
            assert!(
                super::lookup(n).is_some(),
                "{n:?} is listed to run but is not registered — dispatch would panic"
            );
        }
        // Dispatchers are entered by git, not by a list.
        let dispatchers: BTreeSet<&str> =
            ["pre-commit", "pre-push", "commit-msg", "prepare-commit-msg"]
                .into_iter()
                .collect();
        let unreachable: Vec<&str> = REGISTRY
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| !listed.contains(n) && !dispatchers.contains(n))
            .collect();
        assert!(
            unreachable.is_empty(),
            "registered but never run by any dispatcher: {unreachable:?}"
        );
    }

    /// pre-push is serial and fail-fast, so this list IS the cost order.
    #[test]
    fn pre_push_runs_cheapest_first() {
        assert_eq!(
            PRE_PUSH_CHECKS,
            &[
                "pre-push-branch-protect",
                "pre-push-branch-pattern",
                "pre-push-pull-rebase",
                "pre-push-run-tests-js",
            ]
        );
    }

    #[test]
    fn names_are_unique() {
        let mut seen = BTreeSet::new();
        for (n, _) in REGISTRY {
            assert!(seen.insert(*n), "duplicate registration: {n}");
        }
    }
}
