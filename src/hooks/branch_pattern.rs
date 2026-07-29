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

/// Printed verbatim when a name is rejected, so the message keeps telling
/// people the rule in the notation they'd grep for. Kept as the POSIX form the
/// shell version settled on.
const CONTRACT: &str = "^((feat|fix|hotfix|test|automation)/[[:alnum:]_-]+|chore/[[:alnum:]_.-]+)$";

/// Prefixes whose suffix must stay dot-free.
const PLAIN_PREFIXES: [&str; 5] = ["feat", "fix", "hotfix", "test", "automation"];

/// The regex, by hand. `[[:alnum:]]` is ASCII in both rg and `grep -E` under
/// the C locale, so `is_ascii_alphanumeric` is the faithful translation — a
/// Unicode-aware check would ACCEPT names the shell version rejected.
///
/// Dots are allowed only under `chore/`: version-bump branches read naturally
/// (`chore/duro-1.50.50`) while feature names stay dot-free. Git already
/// rejects the dangerous dot forms (`..`, trailing `.lock`).
pub fn conforms(branch: &str) -> bool {
    let Some((prefix, rest)) = branch.split_once('/') else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    let dots_ok = prefix == "chore";
    if !dots_ok && !PLAIN_PREFIXES.contains(&prefix) {
        return false;
    }
    rest.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || (dots_ok && c == '.'))
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
    \u{1b}[38;5;208m{CONTRACT}\u{1b}[0m.
    Rename your branch with: \u{1b}[38;5;208mgit branch -m\u{1b}[0m <branch name>
    Or bypass this check with git -c hook.skip=branch-pattern push"
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
    fn accepts_the_conventional_shapes() {
        assert!(conforms("feat/0-test"));
        assert!(conforms("fix/some_thing"));
        assert!(conforms("hotfix/a-b-c"));
        assert!(conforms("automation/nightly"));
        assert!(conforms("chore/duro-1.50.50")); // dots, under chore only
        assert!(conforms("chore/a_b-1.2"));
    }

    #[test]
    fn rejects_everything_else() {
        assert!(!conforms("feat/duro-1.50.50")); // dots outside chore
        assert!(!conforms("off-pattern")); // no prefix
        assert!(!conforms("duro-1.50.50"));
        assert!(!conforms("feat/")); // empty suffix
        assert!(!conforms("/x"));
        assert!(!conforms("feat/a/b")); // slash isn't in the class
        assert!(!conforms("main"));
        assert!(!conforms("release/1")); // unknown prefix
    }

    /// `[[:alnum:]]` is ASCII under the C locale in both engines the shell
    /// version used. Accepting Unicode letters here would be a silent
    /// LOOSENING of the rule during the port.
    #[test]
    fn alnum_stays_ascii() {
        assert!(!conforms("feat/café"));
        assert!(!conforms("feat/naïve"));
        assert!(!conforms("chore/日本語"));
    }
}
