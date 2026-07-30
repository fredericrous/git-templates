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

/// One check. `Builtin` today; `External` when a declared command implements it.
pub trait Check {
    fn name(&self) -> &str;
    fn stage(&self) -> Stage;
    fn scope(&self) -> Scope;
    fn run(&self, ctx: &Ctx) -> i32;
}

/// A check compiled into the binary.
pub struct Builtin {
    pub name: &'static str,
    pub stage: Stage,
    pub scope: Scope,
    pub run: fn(&Ctx) -> i32,
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
    fn run(&self, ctx: &Ctx) -> i32 {
        (self.run)(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
