//! pre-push-pull-rebase — sync a branch with ITS OWN upstream before pushing,
//! and warn (never act) when the default branch has moved ahead.
//!
//! The hardening in the shell version is the point, and is preserved exactly:
//! an older version ran `git pull --rebase origin HEAD`, where the remote ref
//! `HEAD` resolves to the remote's DEFAULT branch — so every push silently
//! rebased onto main, autostashing uncommitted work. Hence: never touch a dirty
//! tree, rebase only onto the branch's own upstream, and abort cleanly on
//! conflict rather than leaving a half-rebased state.

use crate::git;
use crate::ui::{ERROR_SIGN, VALID_SIGN, WARNING_SIGN};

/// `ahead[[:space:]]+[0-9]+,[[:space:]]*behind` over `git status -sb`.
/// Both counts present means the branch and its upstream have diverged, and an
/// automatic rebase is exactly the wrong move.
pub fn looks_diverged(status: &str) -> bool {
    for line in status.lines() {
        let mut rest = line;
        while let Some(i) = rest.find("ahead") {
            let after = &rest[i + "ahead".len()..];
            let trimmed = after.trim_start_matches([' ', '\t']);
            if trimmed.len() < after.len() {
                let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
                if !digits.is_empty() {
                    let tail = &trimmed[digits.len()..];
                    if let Some(t) = tail.strip_prefix(',') {
                        if t.trim_start_matches([' ', '\t']).starts_with("behind") {
                            return true;
                        }
                    }
                }
            }
            rest = &rest[i + "ahead".len()..];
        }
    }
    false
}

/// `^[[:space:]*]+<name>$` over `git branch` output — the indentation git
/// always prints, plus the `*` marking the current branch.
pub fn lists_branch(branch_list: &str, name: &str) -> bool {
    branch_list.lines().any(|line| {
        let stripped = line.trim_end_matches(['\r']);
        let body = stripped.trim_start_matches([' ', '\t', '*']);
        !body.is_empty() && body.len() < stripped.len() && body == name
    })
}

/// Left side of `git rev-list --left-right --count <a>...<b>`: how far the
/// default branch is ahead of us.
pub fn ahead_count(rev_list: &str) -> Option<u64> {
    rev_list.split_whitespace().next()?.parse().ok()
}

pub fn run(_args: &[std::ffi::OsString]) -> i32 {
    // 1. Never auto-rebase a dirty tree: that autostashes real work and can
    //    leave a broken mid-rebase state during a push.
    if !git::stdout(&["status", "--porcelain"])
        .unwrap_or_default()
        .is_empty()
    {
        println!("{WARNING_SIGN} Uncommitted changes — skipping pre-push pull-rebase.");
        return 0;
    }

    // 2. Only sync a branch that HAS an upstream, and rebase onto that — never
    //    the default branch. A brand-new branch has nothing to sync.
    let Some(upstream) =
        git::stdout(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
    else {
        return 0;
    };

    // 2b. The upstream can be configured locally but GONE on the remote — the
    //     normal state right after a PR squash-merges with delete-on-merge.
    //     `git pull --rebase` would fail on the missing ref and read as a
    //     conflict, wrongly blocking the push.
    let (remote, branch) = upstream.split_once('/').unwrap_or(("origin", &upstream));
    if !git::succeeds(&["ls-remote", "--exit-code", "--heads", remote, branch]) {
        println!(
            "{WARNING_SIGN} Upstream {upstream} no longer exists on the remote (merged + auto-deleted?) — skipping sync."
        );
        return 0;
    }

    // 3. Diverged → warn and DO NOT rebase, but carry on to the default-branch
    //    check below (the shell fell through here too).
    let status = git::stdout(&["status", "-sb"]).unwrap_or_default();
    if looks_diverged(&status) {
        println!("{WARNING_SIGN} Branch diverged from its upstream — skip auto pull-rebase.");
        println!("    Reconcile manually: \u{1b}[38;5;208mgit pull --rebase\u{1b}[0m (or \u{1b}[38;5;208mgit merge\u{1b}[0m)");
    } else if !git::succeeds(&["pull", "--rebase"]) {
        // Abort so the tree is never left half-rebased.
        let _ = git::succeeds(&["rebase", "--abort"]);
        println!("{ERROR_SIGN} pull --rebase hit conflicts (rebase aborted, tree restored).");
        println!("    Resolve manually: \u{1b}[38;5;208mgit pull --rebase\u{1b}[0m");
        return 1;
    } else {
        println!("{VALID_SIGN} Branch is in sync with its upstream");
    }

    // 4. Informational only — never acts.
    let branch_list = git::stdout(&["branch"]).unwrap_or_default();
    let default_branch = if lists_branch(&branch_list, "main") {
        "main"
    } else if lists_branch(&branch_list, "master") {
        "master"
    } else {
        return 0;
    };

    let _ = git::succeeds(&["fetch", "origin", default_branch]);
    let range = format!("origin/{default_branch}...HEAD");
    if let Some(n) =
        git::stdout(&["rev-list", "--left-right", "--count", &range]).and_then(|s| ahead_count(&s))
    {
        if n > 0 {
            // The shell took `head -c 1` of this count — the FIRST CHARACTER —
            // so 12 commits ahead printed "1". The test was only ever
            // non-zero/zero, so the wrong number went unnoticed. Parsed properly
            // here.
            println!("{WARNING_SIGN} origin/{default_branch} is ahead by {n} commit(s).");
            println!("    Consider before merging: \u{1b}[38;5;208mgit merge origin/{default_branch}\u{1b}[0m");
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::{ahead_count, lists_branch, looks_diverged};

    #[test]
    fn detects_divergence_only_when_both_counts_are_present() {
        assert!(looks_diverged(
            "## feat/x...origin/feat/x [ahead 1, behind 2]"
        ));
        assert!(looks_diverged("## a...b [ahead 12,  behind 3]"));
        // ahead only, or behind only, is NOT divergence — a rebase is fine
        assert!(!looks_diverged("## feat/x...origin/feat/x [ahead 3]"));
        assert!(!looks_diverged("## feat/x...origin/feat/x [behind 2]"));
        assert!(!looks_diverged("## feat/x...origin/feat/x"));
        assert!(!looks_diverged("## ahead-of-time...origin/x"));
    }

    #[test]
    fn recognises_git_branch_lines() {
        let list = "  feat/x\n* main\n  master-ish\n";
        assert!(lists_branch(list, "main"));
        assert!(!lists_branch(list, "master")); // master-ish is a different branch
        assert!(!lists_branch(list, "feat")); // must match the whole name
        assert!(lists_branch(list, "feat/x"));
    }

    /// A count of 12 must read as 12, not 1 — the shell's `head -c 1`.
    #[test]
    fn parses_the_full_ahead_count() {
        assert_eq!(ahead_count("12\t3"), Some(12));
        assert_eq!(ahead_count("0\t5"), Some(0));
        assert_eq!(ahead_count(""), None);
    }
}
