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

use crate::check::{Builtin, Check, Outcome, Scope, Severity, Stage};
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
    ("commit-msg", |ctx| hooks::commit_msg::run(ctx.args)),
    ("prepare-commit-msg", |ctx| {
        hooks::prepare_commit_msg::run(ctx.args)
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
        run: |ctx| hooks::k8s::argo_lint(ctx.args),
    },
    Builtin {
        name: "pre-commit-ban-terms",
        stage: Stage::PreCommit,
        scope: Scope::files(&[".js", ".jsx", ".ts", ".tsx", ".vue"]),
        severity: Severity::Block,
        run: |ctx| Outcome::from_code(hooks::ban_terms::run(ctx.name, ctx.args)),
    },
    Builtin {
        name: "pre-commit-cargo-fmt",
        stage: Stage::PreCommit,
        scope: Scope::new(&[".rs"], &["Cargo.toml"]),
        severity: Severity::Block,
        run: |ctx| hooks::rust_tools::fmt(ctx.args),
    },
    Builtin {
        name: "pre-commit-clippy",
        stage: Stage::PreCommit,
        scope: Scope::new(&[".rs"], &["Cargo.toml"]),
        severity: Severity::Block,
        run: |ctx| hooks::rust_tools::clippy(ctx.args),
    },
    Builtin {
        name: "pre-commit-kube-linter",
        stage: Stage::PreCommit,
        scope: Scope::new(
            &[".yaml", ".yml"],
            &[".kube-linter*.yaml", ".kube-linter*.yml"],
        ),
        severity: Severity::Block,
        run: |ctx| hooks::k8s::kube_linter(ctx.args),
    },
    Builtin {
        name: "pre-commit-kubeconform",
        stage: Stage::PreCommit,
        scope: Scope::new(
            &[".yaml", ".yml"],
            &["kustomization.yaml", "kustomization.yml"],
        ),
        severity: Severity::Block,
        run: |ctx| hooks::k8s::kubeconform(ctx.args),
    },
    Builtin {
        name: "pre-commit-lint-js",
        stage: Stage::PreCommit,
        scope: Scope::new(&[".js", ".jsx", ".ts", ".tsx", ".vue"], &["package.json"]),
        severity: Severity::Block,
        run: |ctx| hooks::lint_js::run(ctx.args),
    },
    Builtin {
        name: "pre-commit-lint-json-yaml",
        stage: Stage::PreCommit,
        scope: Scope::files(&[".json", ".yaml", ".yml"]),
        severity: Severity::Block,
        run: |ctx| hooks::lint_json_yaml::run(ctx.args),
    },
    Builtin {
        name: "pre-commit-merge-conflict",
        stage: Stage::PreCommit,
        scope: Scope::ALWAYS,
        severity: Severity::Block,
        run: |ctx| Outcome::from_code(hooks::merge_conflict::run(ctx.name, ctx.args)),
    },
    Builtin {
        name: "pre-commit-package-lock",
        stage: Stage::PreCommit,
        scope: Scope::new(&[], &["package.json"]),
        severity: Severity::Block,
        run: |ctx| hooks::package_lock::run(ctx.args),
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
        run: |ctx| hooks::prettier::run(ctx.args),
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
        run: |ctx| hooks::python_tools::pyright(ctx.args),
    },
    Builtin {
        name: "pre-commit-ruff",
        stage: Stage::PreCommit,
        scope: Scope::new(
            &[".py", ".pyi"],
            &["ruff.toml", ".ruff.toml", "pyproject.toml"],
        ),
        severity: Severity::Block,
        run: |ctx| hooks::python_tools::ruff(ctx.args),
    },
    Builtin {
        name: "pre-commit-usual-name",
        stage: Stage::PreCommit,
        scope: Scope::ALWAYS,
        severity: Severity::Block,
        run: |ctx| hooks::usual_name::run(ctx.args),
    },
    Builtin {
        name: "pre-commit-yamllint",
        stage: Stage::PreCommit,
        scope: Scope::new(
            &[".yaml", ".yml"],
            &[".yamllint.yaml", ".yamllint.yml", ".yamllint"],
        ),
        severity: Severity::Block,
        run: |ctx| hooks::yamllint::run(ctx.args),
    },
    // ---- pre-push, cheapest and most decisive first ----
    Builtin {
        name: "pre-push-branch-protect",
        stage: Stage::PrePush,
        scope: Scope::ALWAYS,
        severity: Severity::Block,
        run: |ctx| Outcome::from_code(hooks::branch_protect::run(ctx.push.get())),
    },
    Builtin {
        name: "pre-push-branch-pattern",
        stage: Stage::PrePush,
        scope: Scope::ALWAYS,
        severity: Severity::Block,
        run: |ctx| Outcome::from_code(hooks::branch_pattern::run(ctx.args)),
    },
    Builtin {
        name: "pre-push-pull-rebase",
        stage: Stage::PrePush,
        scope: Scope::ALWAYS,
        severity: Severity::Block,
        run: |ctx| Outcome::from_code(hooks::pull_rebase::run(ctx.args)),
    },
    Builtin {
        name: "pre-push-run-tests-js",
        stage: Stage::PrePush,
        scope: Scope::new(&[".js", ".jsx", ".ts", ".tsx", ".vue"], &["package.json"]),
        severity: Severity::Block,
        run: |ctx| Outcome::from_code(hooks::run_tests::run(ctx.push.get())),
    },
    Builtin {
        name: "pre-push-cargo-test",
        stage: Stage::PrePush,
        scope: Scope::new(&[".rs"], &["Cargo.toml"]),
        severity: Severity::Block,
        run: |ctx| hooks::rust_tools::test(ctx.push.get()),
    },
];

/// A check's severity, after any per-repository override.
///
/// `git config githooks.severity.<check> warn` downgrades a blocking check to a
/// warning. Unlike `hook.skip` it keeps the signal: the check still runs and
/// still reports, it just stops failing the commit.
pub fn severity_of(check: &dyn Check) -> Severity {
    effective_override(None, check.name()).unwrap_or_else(|| check.severity())
}

/// The config key a severity override lives under.
pub fn severity_key(check: &str) -> String {
    format!("githooks.severity.{check}")
}

/// Every severity override visible here, resolved the way `--get` resolves it.
///
/// ONE subprocess for the whole stage, and — more importantly — a VALUE. The
/// dispatcher used to call `severity_of` inside the loop that classifies
/// outcomes, which put a `git` spawn in the middle of a fold and made the fold
/// impossible to test without a repository.
///
/// `--get-regexp` emits entries in precedence order, so folding with overwrite
/// lands on the same answer `--get` gives. That equivalence is not assumed:
/// `the_batch_agrees_with_the_authority` pins it against `effective_override`.
#[derive(Debug, Default, Clone)]
pub struct Overrides(std::collections::BTreeMap<String, Severity>);

impl Overrides {
    pub fn read() -> Overrides {
        Overrides::from_config(crate::git::stdout(&[
            "config",
            "--get-regexp",
            r"^githooks\.severity\.",
        ]))
    }

    fn from_config(out: Option<String>) -> Overrides {
        let mut map = std::collections::BTreeMap::new();
        for line in out.as_deref().unwrap_or_default().lines() {
            let Some((key, value)) = line.split_once(' ') else {
                continue;
            };
            let Some(check) = key.strip_prefix("githooks.severity.") else {
                continue;
            };
            // Later entries overwrite earlier ones — git's own precedence,
            // observed rather than reimplemented.
            match Severity::parse(value.trim()) {
                Some(s) => {
                    map.insert(check.to_string(), s);
                }
                // An unrecognised value is not an override at all, and must not
                // shadow a valid earlier one either.
                None => {
                    map.remove(check);
                }
            }
        }
        Overrides(map)
    }

    /// The severity to apply to `check`, override or declared.
    pub fn of(&self, check: &dyn Check) -> Severity {
        self.0
            .get(check.name())
            .copied()
            .unwrap_or_else(|| check.severity())
    }
}

/// The override git would actually apply for `check`, or `None` if there is
/// none (or the value is not one this understands).
///
/// `--get`, NOT `--get-regexp`: git returns the LAST value, so a local `block`
/// beats a global `warn`. A reader that listed every entry instead and treated
/// each as authoritative would report a downgrade that the dispatcher does not
/// apply — which is exactly what the dashboard used to do.
///
/// `repo` is `None` for the current directory, which is where a hook runs.
pub fn effective_override(repo: Option<&Path>, check: &str) -> Option<Severity> {
    let key = severity_key(check);
    let value = match repo {
        None => crate::git::stdout(&["config", "--get", &key]),
        Some(dir) => crate::git::stdout_in(dir, &["config", "--get", &key]),
    }?;
    Severity::parse(&value)
}

/// Built-in checks for one stage, in declared order.
pub fn stage_checks(stage: Stage) -> impl Iterator<Item = &'static Builtin> {
    CHECKS.iter().filter(move |check| check.stage == stage)
}

/// Every check for one stage — built-ins first, then whatever this repository
/// declares in `.githooks.conf`.
///
/// The order is not negotiable and not configurable. A third-party command must
/// not be able to delay `pre-push-branch-protect`, and appending is the only
/// arrangement in which it cannot.
pub fn all_stage_checks(stage: Stage) -> Vec<&'static dyn Check> {
    let mut out: Vec<&'static dyn Check> = stage_checks(stage)
        .map(|check| check as &dyn Check)
        .collect();
    out.extend(
        crate::manifest::externals()
            .iter()
            .filter(|external| external.stage == stage)
            .map(|external| external as &dyn Check),
    );
    out
}

pub fn lookup(name: &str) -> Option<HookFn> {
    if let Some((_, f)) = ENTRYPOINTS.iter().find(|(n, _)| *n == name) {
        return Some(*f);
    }
    // A check invoked directly by name — how the tests drive individual checks,
    // and how `githooks <check>` works from a shell. Its Outcome collapses to an
    // exit code here, honouring severity, so a `warn` check invoked directly
    // reports without failing exactly as it does inside a dispatcher.
    if CHECKS.iter().any(|check| check.name == name)
        || crate::manifest::externals()
            .iter()
            .any(|external| external.name == name)
    {
        return Some(|ctx: &Ctx| {
            let check = one_named(ctx.name).expect("checked above");
            match check.run(ctx) {
                Outcome::Failed if severity_of(check) == Severity::Block => 1,
                _ => 0,
            }
        });
    }
    None
}

/// One check by name, whichever kind it is. Built-ins are searched first, which
/// costs nothing because `manifest::parse` refuses a name a built-in already
/// holds — the two guards together mean neither kind can shadow the other.
pub fn one_named(name: &str) -> Option<&'static dyn Check> {
    if let Some(builtin) = CHECKS.iter().find(|check| check.name == name) {
        return Some(builtin);
    }
    crate::manifest::externals()
        .iter()
        .find(|external| external.name == name)
        .map(|external| external as &dyn Check)
}

#[cfg(test)]
mod tests {
    use super::{lookup, Overrides, Severity, Stage, CHECKS, ENTRYPOINTS};
    use std::collections::BTreeSet;

    /// The batch reader must land on the same answer as the authority.
    ///
    /// `Overrides` folds `--get-regexp` output with overwrite; `effective_override`
    /// asks `--get`. They agree only because git emits entries in precedence
    /// order — an assumption, so it is asserted against real git rather than
    /// trusted.
    #[test]
    fn the_batch_agrees_with_the_authority() {
        let d = std::env::temp_dir().join(format!("ov-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&d)
                .output()
                .expect("git");
        };
        git(&["init", "-q", "--template=", "."]);
        let key = "githooks.severity.pre-commit-merge-conflict";
        git(&["config", "--add", key, "warn"]);
        git(&["config", "--add", key, "block"]);

        let raw = std::process::Command::new("git")
            .args(["config", "--get-regexp", r"^githooks\.severity\."])
            .current_dir(&d)
            .output()
            .expect("git");
        let batch = Overrides::from_config(Some(
            String::from_utf8_lossy(&raw.stdout).trim().to_string(),
        ));
        let authority =
            crate::git::stdout_in(&d, &["config", "--get", key]).and_then(|v| Severity::parse(&v));
        let _ = std::fs::remove_dir_all(&d);

        assert_eq!(
            authority,
            Some(Severity::Block),
            "git applies the last entry"
        );
        assert_eq!(
            batch.0.get("pre-commit-merge-conflict").copied(),
            authority,
            "the batch reader disagreed with `--get`"
        );
    }

    /// An unrecognised value is not an override, and must not shadow a valid
    /// earlier one either — a typo would otherwise silently restore the
    /// declared severity in a way nobody could see.
    #[test]
    fn an_unrecognised_value_clears_rather_than_overrides() {
        let o = Overrides::from_config(Some(
            "githooks.severity.a warn\ngithooks.severity.a advisory\ngithooks.severity.b warn"
                .to_string(),
        ));
        assert_eq!(o.0.get("a"), None, "a typo must not leave `warn` standing");
        assert_eq!(o.0.get("b").copied(), Some(Severity::Warn));
    }

    #[test]
    fn names_are_unique_across_entrypoints_and_checks() {
        let mut seen = BTreeSet::new();
        for n in ENTRYPOINTS
            .iter()
            .map(|(n, _)| *n)
            .chain(CHECKS.iter().map(|check| check.name))
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
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
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
        for check in CHECKS {
            assert!(lookup(check.name).is_some(), "{} not reachable", check.name);
        }
        assert!(lookup("pre-commit-not-a-check").is_none());
    }

    /// pre-push is serial and fail-fast, so declaration order IS cost order.
    #[test]
    fn pre_push_runs_cheapest_first() {
        let order: Vec<&str> = super::stage_checks(Stage::PrePush)
            .map(|check| check.name)
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
