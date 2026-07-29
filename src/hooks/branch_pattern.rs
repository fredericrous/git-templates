//! pre-push-branch-pattern — reject branch names that don't follow the
//! convention, unless the branch is already on the remote.
//!
//! Ported from zsh. The shell version needed `rg` with a `grep` fallback and a
//! pattern written to satisfy both engines; here the match is a dozen lines of
//! ASCII checks, so the `rg`/`grep` split and `HOOKS_FORCE_GREP` disappear
//! entirely — as does the failure mode that motivated them, where a missing
//! `rg` made `! rg …` true and the hook rejected EVERY branch name.

use crate::git;
use crate::ui::{ERROR_SIGN, VALID_SIGN};

use crate::vocabulary;

/// Both the rule and the message now come from `vocabulary`, so what a user is
/// told and what is enforced cannot drift — and neither can the branch prefixes
/// and the commit types, which had diverged to the point that `docs/…` was
/// unpushable while `docs:` was a valid commit type.
///
/// `[[:alnum:]]` is ASCII in both rg and `grep -E` under the C locale, which is
/// what the shell version enforced; `is_ascii_alphanumeric` keeps that. A
/// Unicode-aware check would silently LOOSEN the rule.
pub fn conforms(branch: &str) -> bool {
    let Some((prefix, rest)) = branch.split_once('/') else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    let Some(p) = vocabulary::branch_prefix(prefix) else {
        return false;
    };
    rest.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || (p.dots && c == '.'))
}

pub fn run(args: &[std::ffi::OsString]) -> i32 {
    // Unresolvable HEAD (an unborn branch) — nothing to check, don't block.
    let Some(branch) = git::stdout(&["rev-parse", "--abbrev-ref", "HEAD"]) else {
        return 0;
    };

    if git::succeeds(&["show-branch", &format!("remotes/origin/{branch}")]) {
        println!("{VALID_SIGN} Branch already on server. Name is authorized.");
        return 0;
    }

    // Initial push to a brand-new empty remote: there's no feature-branch
    // convention to enforce while initializing a repo, and the default branch
    // (main/master) doesn't match the pattern anyway. git passes the remote
    // name as the first argument.
    let remote = args
        .first()
        .and_then(|a| a.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("origin");
    // `None` here means git failed, which is NOT the same as "no branches" —
    // treat only a successful, empty listing as the initial-push case.
    if git::stdout(&["ls-remote", "--heads", remote]).is_some_and(|s| s.is_empty()) {
        println!("{VALID_SIGN} Remote has no branches yet (initial push). Name is authorized.");
        return 0;
    }

    if !conforms(&branch) {
        println!(
            "{ERROR_SIGN} Branch names in this project must adhere to this contract:
    \u{1b}[38;5;208m{}\u{1b}[0m.
    Rename your branch with: \u{1b}[38;5;208mgit branch -m\u{1b}[0m <branch name>
    Or bypass this check with git -c hook.skip=branch-pattern push",
            vocabulary::branch_contract()
        );
        return 1;
    }

    println!("{VALID_SIGN} Branch name conforms with authorized pattern");
    0
}

#[cfg(test)]
mod tests {
    use super::conforms;

    #[test]
    fn accepts_every_declared_prefix() {
        for p in crate::vocabulary::BRANCH_PREFIXES {
            assert!(conforms(&format!("{}/some-work", p.name)), "{}", p.name);
        }
    }

    /// The divergence this module exists to end: these were all REJECTED as
    /// branch names while being perfectly valid commit types.
    #[test]
    fn accepts_the_prefixes_that_used_to_be_rejected() {
        for b in [
            "docs/rust-migration",
            "refactor/hook-registry",
            "perf/faster-startup",
            "build/bump-toolchain",
            "style/reformat",
            "revert/bad-change",
            "add/new-thing",
            "remove/dead-code",
        ] {
            assert!(conforms(b), "{b} should be allowed now");
        }
    }

    #[test]
    fn rejects_everything_else() {
        assert!(!conforms("off-pattern"));
        assert!(!conforms("duro-1.50.50"));
        assert!(!conforms("feat/"));
        assert!(!conforms("/x"));
        assert!(!conforms("feat/a/b"));
        assert!(!conforms("main"));
        assert!(!conforms("release/1")); // not a declared prefix
    }

    /// Dots stay a chore-only affordance for version bumps.
    #[test]
    fn dots_are_chore_only() {
        assert!(conforms("chore/duro-1.50.50"));
        assert!(!conforms("feat/duro-1.50.50"));
        assert!(!conforms("docs/1.2.3"));
    }

    /// `[[:alnum:]]` is ASCII under the C locale in both engines the shell
    /// version used; a Unicode-aware check would LOOSEN the rule.
    #[test]
    fn alnum_stays_ascii() {
        assert!(!conforms("feat/café"));
        assert!(!conforms("chore/日本語"));
    }
}
