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

use crate::{dispatch, hooks};

/// Everything a hook is given. One shape for all of them, so a handler that
/// needs the invoked name (ban-terms excludes its own source by it) or the
/// hooks directory (the dispatchers glob it) does not need its own signature.
pub struct Ctx<'a> {
    /// The hook name as invoked, suffix already stripped.
    pub name: &'a str,
    /// Arguments git passed the hook.
    pub args: &'a [OsString],
    /// Directory the shim lives in — where sub-hooks are discovered.
    pub hooks_dir: &'a Path,
}

pub type HookFn = fn(&Ctx) -> i32;

/// name → handler. The single place a hook is registered.
pub const REGISTRY: &[(&str, HookFn)] = &[
    ("pre-commit", |c| dispatch::pre_commit(c.hooks_dir, c.args)),
    ("pre-push", |c| dispatch::pre_push(c.hooks_dir, c.args)),
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
    ("pre-push-run-tests-js", |c| hooks::run_tests::run(c.args)),
];

pub fn lookup(name: &str) -> Option<HookFn> {
    REGISTRY.iter().find(|(n, _)| *n == name).map(|(_, f)| *f)
}

#[cfg(test)]
mod tests {
    use super::REGISTRY;
    use std::collections::BTreeSet;

    #[test]
    fn names_are_unique() {
        let mut seen = BTreeSet::new();
        for (n, _) in REGISTRY {
            assert!(seen.insert(*n), "duplicate registration: {n}");
        }
    }

    /// The registry and the shipped shims must describe the same set.
    ///
    /// A shim the binary does not recognise exits 2 and BLOCKS the commit; a
    /// handler with no shim is dead code that ships forever. Both were possible
    /// while the name lived in a match arm and in a filename with nothing
    /// comparing them.
    #[test]
    fn every_shim_has_a_handler_and_vice_versa() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/templates/hooks");
        let mut shims = BTreeSet::new();
        for e in std::fs::read_dir(dir).expect("templates/hooks").flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == "package.json" {
                continue;
            }
            // Only the shims dispatch to the binary; anything else is not ours.
            let body = std::fs::read_to_string(e.path()).unwrap_or_default();
            if !body.contains("--hooks-dir") {
                continue;
            }
            let stem = name.split_once('.').map(|(s, _)| s).unwrap_or(&name);
            shims.insert(stem.to_string());
        }
        let registered: BTreeSet<String> = REGISTRY.iter().map(|(n, _)| n.to_string()).collect();

        let missing: Vec<_> = shims.difference(&registered).collect();
        assert!(
            missing.is_empty(),
            "shims with no handler (exit 2 on use): {missing:?}"
        );
        let orphan: Vec<_> = registered.difference(&shims).collect();
        assert!(
            orphan.is_empty(),
            "handlers with no shim (dead code): {orphan:?}"
        );
    }
}
