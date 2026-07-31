//! What a check IS, as one value rather than four tables.
//!
//! Before this, a check was spread across `REGISTRY` (name → fn), two ordered
//! name lists, and a language table in the fleet crate — four places keyed by
//! the same string, held together by reconciliation tests. Those tests were
//! good, but they policed a shape that should not have been splittable. With
//! the metadata attached to the check, there is nothing left to reconcile.
//!
//! It also gives external checks somewhere to exist. A third party cannot add a
//! Rust module without rebuilding the binary, so extension means a declared
//! command implementing this same trait — and the dispatcher not caring which
//! kind it is holding.

use crate::registry::Ctx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    PreCommit,
    PrePush,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::PreCommit => "pre-commit",
            Stage::PrePush => "pre-push",
        }
    }
}

/// When a check is relevant, declared rather than reimplemented by every
/// reader.
///
/// A CONJUNCTION, not a choice: ruff is `.py` files AND a ruff config; clippy
/// is `.rs` AND `Cargo.toml`. An earlier design offered these as alternatives
/// plus a `Custom` escape hatch, which would have swallowed nearly every check
/// and left the dashboard knowing nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    /// Extensions that trigger it. Empty means any change.
    pub files: &'static [&'static str],
    /// Config paths that opt a repository in. Empty means always on.
    pub opt_in: &'static [&'static str],
}

impl Scope {
    pub const ALWAYS: Scope = Scope {
        files: &[],
        opt_in: &[],
    };

    pub const fn files(files: &'static [&'static str]) -> Scope {
        Scope { files, opt_in: &[] }
    }

    pub const fn new(files: &'static [&'static str], opt_in: &'static [&'static str]) -> Scope {
        Scope { files, opt_in }
    }

    /// Would this check ever fire, given the paths a repository contains?
    ///
    /// Deliberately coarse for checks that resolve an ancestor at run time —
    /// `cargo-fmt` declares `Cargo.toml` meaning "somewhere here" while
    /// enforcing "nearest above the staged file". The dispatcher asks the
    /// precise question by running the check; this answers the dashboard's
    /// question, "would it ever fire", where over-approximating is the safe
    /// direction.
    pub fn matches(&self, paths: &[String]) -> bool {
        let by_ext = self.files.is_empty()
            || paths
                .iter()
                .any(|p| self.files.iter().any(|e| p.ends_with(e)));
        let opted_in = self.opt_in.is_empty()
            || paths.iter().any(|p| {
                let name = p.rsplit('/').next().unwrap_or(p);
                self.opt_in.iter().any(|c| {
                    // A trailing `*` is a prefix match: `.kube-linter*.yaml`.
                    match c.split_once('*') {
                        Some((pre, suf)) => name.starts_with(pre) && name.ends_with(suf),
                        None => name == *c,
                    }
                })
            });
        by_ext && opted_in
    }
}

/// What a check meant, as opposed to what it printed.
///
/// The fourth variant is the point. Fifteen sites used to warn and return 0,
/// collapsing two different situations: "I ran and found something you should
/// know" and "I could not run at all". `ruff config found but no ruff binary`
/// was indistinguishable from ruff running clean — to the dispatcher, and to
/// the dashboard. A repository where a check has silently never executed read
/// as one where it passes.
///
/// That is the same invisibility `hook.skip` had before skipped checks were
/// announced, and it took three PRs to notice there.
///
/// The check still prints its own message; this only classifies the result.
/// `Default` is `Failed` on purpose: it fills the slot of a check whose thread
/// died, and a check that vanished must never read as one that passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Outcome {
    Passed,
    /// Ran, found a problem. Whether that blocks is `Severity`, not this.
    #[default]
    Failed,
    /// Ran, found something worth saying, which does not block.
    Warned,
    /// COULD NOT RUN — a tool is missing, or the opt-in config is absent.
    Unavailable,
}

impl Outcome {
    /// Legacy adapter while checks are converted one at a time.
    pub fn from_code(code: i32) -> Outcome {
        if code == 0 {
            Outcome::Passed
        } else {
            Outcome::Failed
        }
    }
}

/// Whether a failing check stops the commit or merely reports.
///
/// Declared per check and overridable per repository with
/// `git config githooks.severity.<check> warn`. That is a better escape hatch
/// than `hook.skip`, which is all-or-nothing and invisible enough that
/// `hook.skip = e` disables all twenty checks: a downgrade keeps the signal and
/// removes only the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Block,
    Warn,
}

impl Severity {
    /// The ONE mapping from configured text to severity.
    ///
    /// There were four: this key's reader, the manifest's severity column, the
    /// dashboard's copy, and the dashboard's reverse mapping for `--json`. They
    /// agreed, but nothing made them — and the dashboard's copy is its
    /// prediction of what the dispatcher will do, which is the one thing it must
    /// never get wrong.
    ///
    /// `None` for anything else, deliberately: git validates nothing here, so an
    /// unrecognised value must fall back to the declared severity rather than
    /// silently disable a check.
    pub fn parse(value: &str) -> Option<Severity> {
        match value {
            "warn" => Some(Severity::Warn),
            "block" => Some(Severity::Block),
            _ => None,
        }
    }

    /// How it is written in config and in `--json`.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Block => "block",
            Severity::Warn => "warn",
        }
    }
}

/// One check, whether compiled in or declared by the repository.
///
/// `Sync` because `pre-commit` hands every check to its own thread. Both
/// implementations satisfy it for free, and requiring it here is what lets the
/// dispatcher hold `&'static dyn Check` without caring which kind it has.
pub trait Check: Sync {
    fn name(&self) -> &str;
    fn stage(&self) -> Stage;
    fn scope(&self) -> Scope;
    fn severity(&self) -> Severity;
    fn run(&self, ctx: &Ctx) -> Outcome;
}

/// A check compiled into the binary.
pub struct Builtin {
    pub name: &'static str,
    pub stage: Stage,
    pub scope: Scope,
    pub severity: Severity,
    pub run: fn(&Ctx) -> Outcome,
}

impl Check for Builtin {
    fn name(&self) -> &str {
        self.name
    }
    fn stage(&self) -> Stage {
        self.stage
    }
    fn scope(&self) -> Scope {
        self.scope
    }
    fn severity(&self) -> Severity {
        self.severity
    }
    fn run(&self, ctx: &Ctx) -> Outcome {
        (self.run)(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trips, and refuses everything else. A `Some` for an unknown value
    /// would turn a typo into a silent disable.
    #[test]
    fn severity_parses_exactly_the_two_words_it_documents() {
        for s in [Severity::Block, Severity::Warn] {
            assert_eq!(Severity::parse(s.as_str()), Some(s));
        }
        for bad in ["", "Warn", "WARN", "advisory", "true", "1", " warn"] {
            assert_eq!(Severity::parse(bad), None, "{bad:?} must not parse");
        }
    }

    #[test]
    fn always_matches_anything() {
        assert!(Scope::ALWAYS.matches(&[]));
        assert!(Scope::ALWAYS.matches(&["README.md".into()]));
    }

    #[test]
    fn extensions_gate_on_the_file_type() {
        let s = Scope::files(&[".rs"]);
        assert!(s.matches(&["src/main.rs".into()]));
        assert!(!s.matches(&["README.md".into()]));
    }

    /// The case the enum could not express: BOTH conditions must hold.
    #[test]
    fn files_and_opt_in_are_a_conjunction() {
        let ruff = Scope::new(&[".py"], &["ruff.toml", "pyproject.toml"]);
        assert!(
            !ruff.matches(&["a.py".into()]),
            "python alone is not enough — the repo must opt in"
        );
        assert!(
            !ruff.matches(&["pyproject.toml".into()]),
            "and a config alone is not enough without python"
        );
        assert!(ruff.matches(&["a.py".into(), "pyproject.toml".into()]));
    }

    /// `.kube-linter*.yaml` is a real config name in this repo's own hooks.
    #[test]
    fn a_trailing_star_is_a_prefix_match() {
        let s = Scope::new(&[".yaml"], &[".kube-linter*.yaml"]);
        assert!(s.matches(&["k8s/x.yaml".into(), ".kube-linter-prod.yaml".into()]));
        assert!(!s.matches(&["k8s/x.yaml".into(), ".kube-lint.yaml".into()]));
    }

    /// Opt-in matches a BASENAME anywhere, which is what makes the coarse
    /// answer right for a check that resolves an ancestor when it runs.
    #[test]
    fn opt_in_matches_a_nested_manifest() {
        let cargo = Scope::new(&[".rs"], &["Cargo.toml"]);
        assert!(cargo.matches(&["crates/a/src/lib.rs".into(), "crates/a/Cargo.toml".into()]));
    }
}
